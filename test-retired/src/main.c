// roms-test main()

// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#include "roms-test.h"

int main(void) {
    printf("Starting ROM image test...\n");
    printf("-----\n");

    // Load original ROM configurations
    rom_config_t *rom_configs;
    int count = parse_rom_configs(getenv("ROM_CONFIGS"), &rom_configs);
    if (count < 0) {
        printf("Error parsing ROM configurations\n");
        return -1;
    }
    printf("Parsed %d ROM configurations\n", count);

    // Load JSON config
    char *hw_rev = getenv("HW_REV");
    json_config_t *json_config = load_json_config(hw_rev);
    if (json_config == NULL) {
        printf("Error loading JSON configuration\n");
        free_rom_configs(rom_configs, count);
        return -1;
    }
    printf("Loaded hardware configuration: %s: %s\n", hw_rev, json_config->description ? json_config->description : "No description");

    // Load original ROM files
    loaded_rom_t *loaded_roms = NULL;
    int rc = load_all_roms(rom_configs, count, &loaded_roms);
    if (rc) {
        printf("Error loading ROMs: %d\n", rc);
        free_json_config(json_config);
        free_rom_configs(rom_configs, count);
        return -1;
    }

    // Print loaded ROM analysis
    print_loaded_rom_analysis(loaded_roms, rom_configs, count);

    // Print compiled ROM information
    print_compiled_rom_info();

    // Validate the loaded ROMs against the compiled sets
    rc = validate_all_rom_sets(json_config, loaded_roms, rom_configs, count);

    free_all_roms(loaded_roms, count);
    free_json_config(json_config);
    free_rom_configs(rom_configs, count);

    return rc;
}
