#!/usr/bin/env bash

#
# build.sh - Build all MCU variant and config combinations for One ROM project
#
# This script automates building all combinations of MCU variants and configuration
# files for the one-rom project. It supports both CI builds
# (for automated testing), specific test builds (for build system validation), 
# and release builds (with packaging).
#
# Usage:
#   ci/build.sh ci              - Build all image configs
#   ci/build.sh test            - Test option combinations
#   ci/build.sh release v1.2.3  - Build and package for release
#   ci/build.sh clean           - Delete builds/ directory
#
# Configuration files:
#   ci/build-variants.txt       - List of MCU variants to build (one per line)
#   ci/blacklist-config.txt     - Configs to skip building (one per line)
#   ci/size-incompatible.txt    - Config+MCU combinations that don't fit in flash
#   ci/tests.txt                - Specific option combinations to test
#   old-config/*.mk                 - Individual config files to build
#
# Output structure:
#   builds/ci/                  - CI builds
#   builds/test/                - Test builds
#   builds/v1.2.3/              - Release builds
#

set -e  # Exit immediately on any command failure

# Determine script and project directories
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
BLACKLIST_FILENAME="blacklist-config.txt"
BUILD_VARIANTS_FILENAME="build-variants.txt"
SIZE_INCOMPATIBLE_FILENAME="size-incompatible.txt"
TESTS_FILENAME="tests.txt"
BLACKLIST_FILE="${SCRIPT_DIR}/${BLACKLIST_FILENAME}"
BUILD_VARIANTS_FILE="${SCRIPT_DIR}/${BUILD_VARIANTS_FILENAME}"
SIZE_INCOMPATIBLE_FILE="${SCRIPT_DIR}/${SIZE_INCOMPATIBLE_FILENAME}"
TESTS_FILE="${SCRIPT_DIR}/${TESTS_FILENAME}"

#
# Display usage information and exit
#
usage() {
    echo "Usage: $0 <command> [args]"
    echo "Commands:"
    echo "  ci                - Build firmware for all image configs"
    echo "  test              - Test option combinations"
    echo "  release <version> - Build and package for release (e.g. v1.2.3)"
    echo "  clean             - Delete builds/ directory"
    echo ""
    echo "Configuration files:"
    echo "  ci/${BUILD_VARIANTS_FILENAME}      - MCU variants to build"
    echo "  ci/${BLACKLIST_FILENAME}  - Configs to skip"
    echo "  ci/${SIZE_INCOMPATIBLE_FILENAME} - Size-incompatible combinations"
    echo "  ci/${TESTS_FILENAME}             - Specific test combinations"
    exit 1
}

#
# Parse comma-separated variables respecting quoted strings
# Args: comma_separated_string
# Returns: Array of variable assignments (one per line)
#
parse_vars_with_quotes() {
    local input="$1"
    local result=()
    local current=""
    local in_quotes=0
    
    for ((i=0; i<${#input}; i++)); do
        local char="${input:i:1}"
        
        if [[ "$char" == '"' ]]; then
            in_quotes=$((1 - in_quotes))
            current+="$char"
        elif [[ "$char" == ',' && $in_quotes -eq 0 ]]; then
            [[ -n "$current" ]] && result+=("$current")
            current=""
        else
            current+="$char"
        fi
    done
    
    [[ -n "$current" ]] && result+=("$current")
    printf '%s\n' "${result[@]}"
}

#
# Remove the entire builds/ directory
#
clean_builds() {
    echo "Cleaning builds directory..."
    rm -rf "${PROJECT_ROOT}/builds"
    echo "Done."
}

#
# Get the correct binary prefix for an MCU variant
# Args: mcu_variant
# Returns: correct binary prefix (e.g. "sdrr-stm32f401re" or "sdrr-rp2350")
#
get_bin_prefix() {
    local mcu="$1"
    if [[ "$mcu" =~ ^f.* ]]; then
        echo "sdrr-stm32${mcu}"
    else
        echo "sdrr-${mcu}"
    fi
}

#
# Load MCU variants from build-variants.txt file
# Returns: Array of variant names (one per line)
#
load_mcu_variants() {
    local variants=()
    if [[ ! -f "${BUILD_VARIANTS_FILE}" ]]; then
        echo "ERROR: MCU variants file not found: ${BUILD_VARIANTS_FILE}"
        exit 1
    fi
    
    while IFS= read -r line; do
        # Skip empty lines and comments (lines starting with #)
        [[ -z "$line" || "$line" =~ ^[[:space:]]*# ]] && continue
        variants+=("$line")
    done < "${BUILD_VARIANTS_FILE}"
    
    if [[ ${#variants[@]} -eq 0 ]]; then
        echo "ERROR: No MCU variants found in ${BUILD_VARIANTS_FILE}"
        exit 1
    fi
    
    printf '%s\n' "${variants[@]}"
}

#
# Parse MCU variant line to extract MCU name and environment variables
# Args: mcu_variant_line (format: "mcu_name" or "mcu_name:VAR1=val1,VAR2=val2")
# Returns: Prints "mcu_name" on first line, then env vars on subsequent lines
#
parse_mcu_variant() {
    local variant_line="$1"
    
    if [[ "$variant_line" =~ : ]]; then
        # Parse MCU name and env vars
        IFS=':' read -r mcu_name env_vars <<< "$variant_line"
        echo "$mcu_name"
        
        if [[ -n "$env_vars" ]]; then
            IFS=',' read -ra var_pairs <<< "$env_vars"
            for var_pair in "${var_pairs[@]}"; do
                echo "$var_pair"
            done
        fi
    else
        # Just MCU name, no env vars
        echo "$variant_line"
    fi
}

#
# Load blacklisted configs from blacklist-config.txt file
# Returns: Array of config names to skip building
#
load_blacklist() {
    local blacklist=()
    if [[ -f "${BLACKLIST_FILE}" ]]; then
        while IFS= read -r line; do
            # Skip empty lines and comments (lines starting with #)
            [[ -z "$line" || "$line" =~ ^[[:space:]]*# ]] && continue
            blacklist+=("$line")
        done < "${BLACKLIST_FILE}"
    fi
    printf '%s\n' "${blacklist[@]}"
}

#
# Load size-incompatible combinations from size-incompatible.txt file
# Returns: Lines in format "config_name mcu1 mcu2 ..."
#
load_size_incompatible() {
    local incompatible=()
    if [[ -f "${SIZE_INCOMPATIBLE_FILE}" ]]; then
        while IFS= read -r line; do
            # Skip empty lines and comments (lines starting with #)
            [[ -z "$line" || "$line" =~ ^[[:space:]]*# ]] && continue
            incompatible+=("$line")
        done < "${SIZE_INCOMPATIBLE_FILE}"
    fi
    printf '%s\n' "${incompatible[@]}"
}

#
# Check if a config+MCU combination is size-incompatible
# Args: config_name, mcu_variant, incompatible_lines_array...
# Returns: 0 if incompatible, 1 if compatible
#
is_size_incompatible() {
    local config="$1"
    local mcu="$2"
    local incompatible_lines=("${@:3}")
    
    for line in "${incompatible_lines[@]}"; do
        # Parse line: first word is config, rest are MCU variants
        read -ra parts <<< "$line"
        local line_config="${parts[0]}"
        local line_mcus=("${parts[@]:1}")
        
        # Check if this line matches our config
        if [[ "$config" == "$line_config" ]]; then
            # Check if our MCU variant is in the incompatible list
            for incompatible_mcu in "${line_mcus[@]}"; do
                if [[ "$mcu" == "$incompatible_mcu" ]]; then
                    return 0  # Found incompatible combination
                fi
            done
        fi
    done
    return 1  # Not incompatible
}

#
# Check if a config is in the blacklist
# Args: config_name, blacklist_array...
# Returns: 0 if blacklisted, 1 if not
#
is_blacklisted() {
    local config="$1"
    local blacklist_array=("${@:2}")
    
    for blacklisted in "${blacklist_array[@]}"; do
        if [[ "$config" == "$blacklisted" ]]; then
            return 0  # Found in blacklist
        fi
    done
    return 1  # Not blacklisted
}

#
# Discover all available config files in old-config/ directory
# Returns: Array of config names (without .mk extension)
#
discover_configs() {
    local configs=()
    
    # Find .mk files in old-config/ (top level only)
    for mk_file in "${PROJECT_ROOT}/old-config"/*.mk; do
        if [[ -f "$mk_file" ]]; then
            local config_name=$(basename "$mk_file" .mk)
            configs+=("$config_name")
        fi
    done
    
    # Find .mk files in old-config/third-party/
    if [[ -d "${PROJECT_ROOT}/old-config/third-party" ]]; then
        for mk_file in "${PROJECT_ROOT}/old-config/third-party"/*.mk; do
            if [[ -f "$mk_file" ]]; then
                local config_name="third-party/$(basename "$mk_file" .mk)"
                configs+=("$config_name")
            fi
        done
    fi
    
    if [[ ${#configs[@]} -eq 0 ]]; then
        echo "ERROR: No config files found in ${PROJECT_ROOT}/old-config/"
        exit 1
    fi
    
    printf '%s\n' "${configs[@]}"
}

#
# Load test combinations from tests.txt file
# Returns: Lines in format "test_name:MCU=variant,VAR1=val1,CONFIG=old-config/file.mk"
#
load_tests() {
    local tests=()
    if [[ ! -f "${TESTS_FILE}" ]]; then
        echo "ERROR: Tests file not found: ${TESTS_FILE}"
        exit 1
    fi
    
    while IFS= read -r line; do
        # Skip empty lines and comments (lines starting with #)
        [[ -z "$line" || "$line" =~ ^[[:space:]]*# ]] && continue
        tests+=("$line")
    done < "${TESTS_FILE}"
    
    if [[ ${#tests[@]} -eq 0 ]]; then
        echo "ERROR: No tests found in ${TESTS_FILE}"
        exit 1
    fi
    
    printf '%s\n' "${tests[@]}"
}

#
# Build a single MCU variant + config combination
# Args: mcu_variant, config_name
# Returns: 0 on success, 1 on failure
#
build_combination() {
    local mcu_line="$1"
    local config="$2"
    local config_file="old-config/${config}.mk"
    
    # Parse MCU variant line
    mapfile -t parsed < <(parse_mcu_variant "$mcu_line")
    local mcu="${parsed[0]}"
    local env_vars=("${parsed[@]:1}")
    
    echo "Building MCU=${mcu} CONFIG=${config_file}"
    if [[ ${#env_vars[@]} -gt 0 ]]; then
        echo "  with env vars: ${env_vars[*]}"
    fi
    
    # Clean firmware and generated files, but preserve Rust generator to avoid rebuild
    make clean-firmware-build clean-gen > /dev/null 2>&1 || true

    # Retry logic: Attempt the build up to 2 times if it fails
    local attempt=1
    local max_attempts=2
    local success=0

    while [[ $attempt -le $max_attempts ]]; do
        echo "    - Attempt $attempt: Building MCU=${mcu} CONFIG=${config_file}"
        
        # Build make command with env vars
        local make_cmd="MCU=\"$mcu\" CONFIG=\"$config_file\""
        for env_var in "${env_vars[@]}"; do
            make_cmd="$env_var $make_cmd"
        done
        make_cmd="$make_cmd make gen firmware info"
        
        if eval "$make_cmd" > /dev/null; then
            success=1
            break
        else
            echo "Build failed on attempt $attempt"
        fi
        attempt=$((attempt + 1))
    done

    if [[ $success -eq 0 ]]; then
        echo "ERROR: Build failed after $max_attempts attempts"
        return 1
    fi

    # Verify that all expected output files were created
    local build_dir="${PROJECT_ROOT}/sdrr/build"
    local base_name="$(get_bin_prefix "$mcu")"
    
    for ext in bin dis elf map; do
        if [[ ! -f "${build_dir}/${base_name}.${ext}" ]]; then
            echo "ERROR: Expected file ${base_name}.${ext} not found"
            return 1
        fi
    done
    
    return 0
}

#
# Execute a single test combination
# Args: test_line (format: "test_name:MCU=variant,VAR1=val1,CONFIG=old-config/file.mk")
# Returns: 0 on success, 1 on failure
#
execute_test() {
    local test_line="$1"
    local test_num="$2"

    # Parse test name and variables
    IFS=':' read -r test_name test_vars <<< "$test_line"
    
    # Parse environment variables
    local env_vars=()
    local mcu_variant=""
    local config_param=""
    
    if [[ -n "$test_vars" ]]; then
        mapfile -t var_pairs < <(parse_vars_with_quotes "$test_vars")
        for var_pair in "${var_pairs[@]}"; do
            if [[ "$var_pair" =~ ^MCU= ]]; then
                mcu_variant="${var_pair#MCU=}"
            elif [[ "$var_pair" =~ ^CONFIG= ]]; then
                config_param="$var_pair"
            else
                env_vars+=("$var_pair")
            fi
        done
    fi
    
    # Validate MCU variant is specified
    if [[ -z "$mcu_variant" ]]; then
        echo "ERROR: Test '$test_name' missing MCU variant"
        return 1
    fi
    
    echo "- Running test $test_num: $test_name (MCU=$mcu_variant)"
    
    # Clean previous build artifacts
    make clean-firmware-build clean-gen > /dev/null 2>&1 || true
    
    # Build command with environment variables
    local make_cmd="MCU=$mcu_variant"
    for env_var in "${env_vars[@]}"; do
        make_cmd="$env_var $make_cmd"
    done
    if [[ -n "$config_param" ]]; then
        make_cmd="$make_cmd $config_param"
    fi
    echo "  - Command: $make_cmd make test"
    make_cmd="$make_cmd make test"
    
    # Retry logic: Attempt the test up to 2 times if it fails
    local attempt=1
    local max_attempts=2
    local success=0

    while [[ $attempt -le $max_attempts ]]; do
        echo "    - Attempt $attempt: Executing test '$test_name'"

        local output
        if output=$(eval "$make_cmd" 2>&1); then
            success=1
            break
        else
            echo "Test '$test_name' failed on attempt $attempt"
            echo "$output"
        fi

        attempt=$((attempt + 1))
    done

    if [[ $success -eq 0 ]]; then
        echo "ERROR: Test '$test_name' failed after $max_attempts attempts"
        return 1
    fi

    return 0
}

#
# Move build artifacts from sdrr/build/ to organized directory structure
# Args: mcu_variant, config_name, target_base_dir
#
organize_files() {
    local mcu_line="$1"
    local config="$2"
    local target_dir="$3"
    
    # Parse MCU and env vars
    mapfile -t parsed < <(parse_mcu_variant "$mcu_line")
    local mcu="${parsed[0]}"
    local env_vars=("${parsed[@]:1}")
    
    # Extract HW_REV from env_vars
    local hw_rev=""
    for env_var in "${env_vars[@]}"; do
        if [[ "$env_var" =~ ^HW_REV= ]]; then
            hw_rev="${env_var#HW_REV=}"
            break
        fi
    done
    
    # Validate hw_rev exists
    if [[ -z "$hw_rev" ]]; then
        echo "ERROR: No HW_REV specified for MCU $mcu"
        exit 1
    fi
    
    local build_dir="${PROJECT_ROOT}/sdrr/build"
    local base_name="$(get_bin_prefix "$mcu")"
    local dest_dir="${target_dir}/${hw_rev}/${mcu}/${config}"
    
    mkdir -p "$dest_dir"
    
    # Copy only .bin files
    cp "${build_dir}/${base_name}.bin" "$dest_dir/"
}

#
# Create zip files for release distribution
# Args: version_string
#
create_zips() {
    local version="$1"
    local builds_dir="${PROJECT_ROOT}/builds/${version}"
    
    echo "Creating zip files..."
    
    # Create firmware directory
    mkdir -p "${builds_dir}/firmware"
    
    # Create a temp directory for organizing files before zipping
    local temp_dir=$(mktemp -d)
    
    # Iterate through all hw_rev directories
    for hw_rev_dir in "${builds_dir}"/*; do
        # Skip if not a directory or if it's the firmware directory itself
        [[ ! -d "$hw_rev_dir" || "$(basename "$hw_rev_dir")" == "firmware" ]] && continue
        
        local hw_rev=$(basename "$hw_rev_dir")
        
        # Iterate through all MCU directories within this hw_rev
        for mcu_dir in "${hw_rev_dir}"/*; do
            [[ ! -d "$mcu_dir" ]] && continue
            
            local mcu=$(basename "$mcu_dir")
            local zip_name="onerom-${hw_rev}-${mcu}.zip"
            
            # Create temp subdir for this zip's contents
            local zip_temp="${temp_dir}/${hw_rev}-${mcu}"
            mkdir -p "$zip_temp"
            
            # Find all .bin files recursively
            while IFS= read -r bin_file; do
                # Get config path relative to mcu_dir
                local rel_path="${bin_file#${mcu_dir}/}"
                # Extract directory path (config with subdirs)
                local config_dir=$(dirname "$rel_path")
                # Convert path to dashed format: third-party/whatever -> third-party-whatever
                local config="${config_dir//\//-}"
                
                local bin_prefix="$(get_bin_prefix "$mcu")"
                local new_name="onerom-${hw_rev}-${mcu}-${config}.bin"
                
                # Copy to temp for zipping
                cp "$bin_file" "${zip_temp}/${new_name}"
                # Also copy directly to firmware directory as individual artifact
                cp "$bin_file" "${builds_dir}/firmware/${new_name}"
            done < <(find "$mcu_dir" -name "*.bin" -type f)
            
            # Create zip from temp directory if it has files
            if [[ -n "$(ls -A "$zip_temp" 2>/dev/null)" ]]; then
                cd "$zip_temp"
                zip "${builds_dir}/firmware/${zip_name}" *.bin > /dev/null
                echo "Created firmware/${zip_name}"
                cd - > /dev/null
            fi
        done
    done
    
    # Clean up temp directory
    rm -rf "$temp_dir"
    
    # Return to project root
    cd "${PROJECT_ROOT}"
}

#
# Main script execution logic
#
main() {
    # Require at least one argument
    [[ $# -lt 1 ]] && usage
    
    local command="$1"
    
    # Handle different command modes
    case "$command" in
        clean)
            clean_builds
            exit 0
            ;;
        ci)
            local target_dir="${PROJECT_ROOT}/builds/ci"
            local mode="ci"
            ;;
        test)
            local target_dir="${PROJECT_ROOT}/builds/test"
            local mode="test"
            ;;
        release)
            # Release mode requires version argument
            [[ $# -ne 2 ]] && usage
            local version="$2"
            local target_dir="${PROJECT_ROOT}/builds/${version}"
            local mode="release"
            ;;
        *)
            usage
            ;;
    esac
    
    # Change to project root directory for make commands
    cd "${PROJECT_ROOT}"
    
    # Execute based on mode
    if [[ "$mode" == "test" ]]; then
        # Test mode: Execute specific test combinations and organize files for verification

        # Initial clean to ensure we start with a clean slate
        echo "Performing initial clean..."
        make clean > /dev/null 2>&1 || true

        # Build sdrr-gen once
        echo "Building sdrr-gen (this will take a little while)..."
        make sdrr-gen
    
        mkdir -p "$target_dir"
        
        mapfile -t tests < <(load_tests)
        
        echo "Found tests: ${#tests[@]}"
        echo "First test will take longer as it builds sdrr-gen"
        echo ""
        
        local total_tests=0
        local successful_tests=0
        
        for test_line in "${tests[@]}"; do
            total_tests=$((total_tests + 1))
            
            if execute_test "$test_line" "$total_tests"; then
                # Extract MCU and HW_REV from test line
                IFS=':' read -r test_name test_vars <<< "$test_line"
                local mcu_variant=""
                local hw_rev=""
                
                if [[ -n "$test_vars" ]]; then
                    IFS=',' read -ra var_pairs <<< "$test_vars"
                    for var_pair in "${var_pairs[@]}"; do
                        if [[ "$var_pair" =~ ^MCU= ]]; then
                            mcu_variant="${var_pair#MCU=}"
                        elif [[ "$var_pair" =~ ^HW_REV= ]]; then
                            hw_rev="${var_pair#HW_REV=}"
                        fi
                    done
                fi
                
                # Reconstruct variant line format for organize_files
                local variant_line="${mcu_variant}:HW_REV=${hw_rev}"
                organize_files "$variant_line" "$test_name" "$target_dir"

                successful_tests=$((successful_tests + 1))
            else
                echo "ERROR: Test failed, stopping"
                exit 1
            fi
        done
        
        # Display test results summary
        echo ""
        echo "Test summary: ${successful_tests}/${total_tests} successful"
        echo "Test build validation complete"
        echo "Files organized in: $target_dir"
        
    elif [[ "$mode" == "release" ]]; then
        # Release mode: Package existing CI build
        local ci_dir="${PROJECT_ROOT}/builds/ci"
        
        # Verify CI build exists
        if [[ ! -d "$ci_dir" ]]; then
            echo "ERROR: No CI build found at ${ci_dir}. Run 'ci/build.sh ci' first."
            exit 1
        fi
        
        # Remove existing release directory and copy from CI
        echo "Copying CI build to release directory..."
        rm -rf "$target_dir"

        # Verify CI build exists
        if [[ ! -d "$ci_dir" ]]; then
            echo "ERROR: No CI build found at ${ci_dir}. Run 'ci/build.sh ci' first."
            exit 1
        fi
        cp -r "$ci_dir" "$target_dir"
        
        # Create zip files
        create_zips "$version"
        generate_manifest "$version"
        echo "Release ${version} packaging complete"
        echo "Firmware zips: ${target_dir}/firmware/*-firmware.zip"
        echo "Complete zips: ${target_dir}/full-output/*-all.zip"
        
    else
        # CI mode: Build all combinations

        # Initial clean to ensure we start with a clean slate
        echo "Performing initial clean..."
        make clean > /dev/null 2>&1 || true

        # Build sdrr-gen once
        echo "Building sdrr-gen (this will take a little while)..."
        make sdrr-gen

        # Ensure target directory exists
        mkdir -p "$target_dir"
        
        # Load configuration data
        mapfile -t blacklist < <(load_blacklist)
        mapfile -t configs < <(discover_configs)
        mapfile -t mcu_variants < <(load_mcu_variants)
        mapfile -t size_incompatible < <(load_size_incompatible)
        
        # Display configuration summary
        echo "Found MCU variants: ${mcu_variants[*]}"
        echo "Found configs: ${configs[*]}"
        if [[ ${#blacklist[@]} -gt 0 ]]; then
            echo "Blacklisted configs: ${blacklist[*]}"
        fi
        echo ""
        
        # Build all valid combinations
        local total_combinations=0
        local successful_builds=0
        local blacklisted_skips=0
        local size_incompatible_skips=0
        
        for mcu_line in "${mcu_variants[@]}"; do
            # Parse MCU name from variant line
            mapfile -t parsed_mcu < <(parse_mcu_variant "$mcu_line")
            local mcu="${parsed_mcu[0]}"
            
            for config in "${configs[@]}"; do
                total_combinations=$((total_combinations + 1))
                
                # Skip blacklisted configurations
                if is_blacklisted "$config" "${blacklist[@]}"; then
                    echo "Skipping blacklisted config: $config (MCU: $mcu)"
                    blacklisted_skips=$((blacklisted_skips + 1))
                    continue
                fi

                # Check pin-count compatibility based on naming
                if [[ "$config" =~ ^28- ]] && [[ ! "$mcu_line" =~ -28- ]]; then
                    echo "Skipping pin-incompatible: 28-pin config $config with non-28-pin MCU $mcu"
                    pin_incompatible_skips=$((pin_incompatible_skips + 1))
                    continue
                fi
                if [[ ! "$config" =~ ^28- ]] && [[ "$mcu_line" =~ -28- ]]; then
                    echo "Skipping pin-incompatible: non-28-pin config $config with 28-pin MCU $mcu"
                    pin_incompatible_skips=$((pin_incompatible_skips + 1))
                    continue
                fi
                
                # Skip size-incompatible combinations
                if is_size_incompatible "$config" "$mcu" "${size_incompatible[@]}"; then
                    echo "Skipping size-incompatible combination: $config + $mcu"
                    size_incompatible_skips=$((size_incompatible_skips + 1))
                    continue
                fi
                
                # Attempt to build this combination
                if build_combination "$mcu_line" "$config"; then
                    # Build succeeded, organize the output files
                    organize_files "$mcu_line" "$config" "$target_dir"
                    successful_builds=$((successful_builds + 1))
                else
                    echo "ERROR: Build failed for MCU=${mcu} CONFIG=${config}"
                    exit 1
                fi
            done
        done
        
        # Display build results summary
        local attempted_builds=$((total_combinations - blacklisted_skips - size_incompatible_skips))
        echo ""
        echo "Build summary:"
        echo "  Total combinations: ${total_combinations}"
        echo "  Blacklisted skips: ${blacklisted_skips}"
        echo "  Size-incompatible skips: ${size_incompatible_skips}"
        echo "  Attempted builds: ${attempted_builds}"
        echo "  Successful builds: ${successful_builds}"
        echo "CI build complete"
        echo "Files organized in: $target_dir"
    fi
}

#
# Generate manifest JSON for release artifacts
# Args: version_string
#
generate_manifest() {
    local version="$1"
    local builds_dir="${PROJECT_ROOT}/builds/${version}"
    local firmware_dir="${builds_dir}/firmware"
    local manifest_file="${firmware_dir}/manifest.json"
    
    echo "Generating manifest..."
    
    # Get GitHub repo info (try from git remote, fallback to env vars)
    local github_repo="${GITHUB_REPOSITORY:-}"
    if [[ -z "$github_repo" ]]; then
        # Try to extract from git remote
        local git_remote=$(git remote get-url origin 2>/dev/null || echo "")
        if [[ "$git_remote" =~ github.com[:/]([^/]+/[^/.]+) ]]; then
            github_repo="${BASH_REMATCH[1]}"
            # Remove .git suffix if present
            github_repo="${github_repo%.git}"
        fi
    fi
    
    if [[ -z "$github_repo" ]]; then
        echo "ERROR: Could not determine GitHub repository. Set GITHUB_REPOSITORY env var or ensure git remote is configured."
        exit 1
    fi
    
    # Function to get model from MCU
    get_model() {
        local mcu="$1"
        if [[ "$mcu" =~ ^rp.* ]]; then
            echo "fire"
        elif [[ "$mcu" =~ ^f.* ]]; then
            echo "ice"
        else
            echo "unknown"
        fi
    }
    
    # Function to get display name for hardware revision
    get_hw_display() {
        local hw_rev="$1"
        case "$hw_rev" in
            24-h) echo "Ice 24 H" ;;
            24-g) echo "Ice 24 G" ;;
            24-f) echo "Ice 24 F" ;;
            ice-24-f) echo "Ice 24 F" ;;
            ice-24-g) echo "Ice 24 G" ;;
            ice-24-h) echo "Ice 24 H" ;;
            ice-24-i) echo "Ice 24 I" ;;
            ice-24-j) echo "Ice 24 J" ;;
            ice-24-usb-h) echo "Ice 24 H" ;;
            p24-a) echo "Fire 24 A" ;;
            fire-24-a) echo "Fire 24 A" ;;
            fire-24-usb-b) echo "Fire 24 B" ;;
            fire-24-c) echo "Fire 24 C" ;;
            fire-24-d) echo "Fire 24 D" ;;
            fire-24-e) echo "Fire 24 E" ;;
            fire-28-a) echo "Fire 28 A" ;;
            fire-28-b) echo "Fire 28 B" ;;
            fire-28-c) echo "Fire 28 C" ;;
            fire-32-a) echo "Fire 32 A" ;;
            fire-32-b) echo "Fire 32 B" ;;
            fire-40-a) echo "Fire 40 A" ;;
            *) echo "$hw_rev" ;;  # Default to hw_rev itself
        esac
    }
    
    # Start building JSON
    local hardware_json="{"
    local artifacts_json="["
    local first_hw=true
    local first_artifact=true
    
    # Collect hardware metadata
    for hw_rev_dir in "${builds_dir}"/*; do
        [[ ! -d "$hw_rev_dir" || "$(basename "$hw_rev_dir")" == "firmware" ]] && continue
        
        local hw_rev=$(basename "$hw_rev_dir")
        
        # Read hardware config JSON if it exists
        local hw_config_file="${PROJECT_ROOT}/rust/config/json/${hw_rev}.json"
        if [[ -f "$hw_config_file" ]]; then
            local description=$(jq -r '.description // ""' "$hw_config_file")
            local usb_support=$(jq -r '.mcu.usb.present // false' "$hw_config_file")
            local display=$(get_hw_display "$hw_rev")
            
            # Add to hardware JSON
            if [[ "$first_hw" == true ]]; then
                first_hw=false
            else
                hardware_json+=","
            fi
            
            hardware_json+="\"${hw_rev}\":{\"display\":\"${display}\",\"description\":\"${description}\",\"usb_support\":${usb_support}}"
        fi
    done
    hardware_json+="}"

    # Build rom_configs JSON - scan all .mk files including subdirectories
    local rom_configs_json="{"
    local first_config=true

    while IFS= read -r config_file; do
        # Get path relative to old-config/ directory and strip .mk
        local rel_path="${config_file#${PROJECT_ROOT}/old-config/}"
        local config_name="${rel_path%.mk}"
        local description=$(extract_config_description "$config_file")
        
        if [[ "$first_config" == true ]]; then
            first_config=false
        else
            rom_configs_json+=","
        fi
        
        # Use dashed format for JSON key: third-party/whatever -> third-party-whatever
        local config_key="${config_name//\//-}"
        rom_configs_json+="\"${config_key}\":{\"description\":\"${description}\"}"
    done < <(find "${PROJECT_ROOT}/old-config" -name "*.mk" -type f)
    rom_configs_json+="}"
    
    # Build artifacts list - find all .bin files recursively
    for hw_rev_dir in "${builds_dir}"/*; do
        [[ ! -d "$hw_rev_dir" || "$(basename "$hw_rev_dir")" == "firmware" ]] && continue
        
        local hw_rev=$(basename "$hw_rev_dir")
        
        for mcu_dir in "${hw_rev_dir}"/*; do
            [[ ! -d "$mcu_dir" ]] && continue
            
            local mcu=$(basename "$mcu_dir")
            local model=$(get_model "$mcu")
            
            # Find all .bin files recursively under this MCU directory
            while IFS= read -r bin_file; do
                # Get config path relative to mcu_dir
                local rel_path="${bin_file#${mcu_dir}/}"
                local config_dir=$(dirname "$rel_path")
                # Convert to dashed format
                local config="${config_dir//\//-}"
                
                local filename="onerom-${hw_rev}-${mcu}-${config}.bin"
                local filepath="${firmware_dir}/${filename}"
                local url="https://github.com/${github_repo}/releases/download/${version}/${filename}"
                
                # Calculate SHA256 checksum
                local sha256=""
                if [[ -f "$filepath" ]]; then
                    sha256=$(sha256sum "$filepath" | awk '{print $1}')
                fi
                
                # Add to artifacts JSON
                if [[ "$first_artifact" == true ]]; then
                    first_artifact=false
                else
                    artifacts_json+=","
                fi
                
                artifacts_json+="{\"hw_rev\":\"${hw_rev}\",\"mcu\":\"${mcu}\",\"model\":\"${model}\",\"rom_config\":\"${config}\",\"filename\":\"${filename}\",\"url\":\"${url}\",\"sha256\":\"${sha256}\"}"
            done < <(find "$mcu_dir" -name "*.bin" -type f)
        done
    done
    artifacts_json+="]"
    
    # Build models JSON (static for now)
    local models_json="{\"ice\":{\"display\":\"Ice\"},\"fire\":{\"display\":\"Fire\"}}"
    
    # Combine into final manifest
    local manifest="{\"version\":\"${version}\",\"hardware\":${hardware_json},\"models\":${models_json},\"rom_configs\":${rom_configs_json},\"artifacts\":${artifacts_json}}"
    
    # Pretty-print and save
    echo "$manifest" | jq '.' > "$manifest_file"
    
    echo "Generated manifest: ${manifest_file}"
}

#
# Extract description from config .mk file
# Args: config_file_path
# Returns: description text (multi-line comments from top of file)
#
extract_config_description() {
    local config_file="$1"
    local description=""
    local in_header=true
    
    while IFS= read -r line; do
        # Skip empty lines at start
        if [[ -z "$line" ]] && [[ -z "$description" ]]; then
            continue
        fi
        
        # If line starts with #, extract the comment
        if [[ "$line" =~ ^#[[:space:]]?(.*) ]]; then
            local text="${BASH_REMATCH[1]}"
            if [[ -n "$description" ]]; then
                description+="\\n"
            fi
            description+="$text"
        # Stop at first non-comment, non-empty line
        elif [[ -n "$line" ]]; then
            break
        fi
    done < "$config_file"
    
    echo "$description"
}

# Execute main function with all script arguments
main "$@"