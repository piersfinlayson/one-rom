// Functions to load and parse the orginal rom configuration

// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#include "roms-test.h"

// Parse ROM_CONFIGS string and return array of rom_config_t
// Returns number of ROMs parsed, or -1 on error
// Caller must free the returned array and strings within it
int parse_rom_configs(const char *configs, rom_config_t **roms) {
    if (!configs) {
        printf("Error: No ROM_CONFIGS\n");
        return -1;
    }

    assert(roms != NULL);

    // Count ROMs first (count spaces/backslashes + 1)
    int count = 1;
    for (const char *p = configs; *p; p++) {
        if (*p == ' ') count++;
    }

    // Make a working copy since strtok modifies the string
    char *work = strdup(configs);
    if (!work) return -1;

    // Allocate ROM array
    *roms = calloc(count, sizeof(rom_config_t));
    if (!*roms) {
        free(work);
        return -1;
    }
    
    // Initialize all optional fields to -1 (not specified)
    for (int i = 0; i < count; i++) {
        (*roms)[i].cs2 = -1;
        (*roms)[i].cs3 = -1;
        (*roms)[i].set = -1;
        (*roms)[i].dup = 0;
        (*roms)[i].pad = 0;
    }
    
    char *outer_state, *inner_state;
    int rom_idx = 0;
    char *rom_str = strtok_r(work, " ", &outer_state);
    
    while (rom_str && rom_idx < count) {
        // Parse each ROM config: "file=...,type=...,cs1=..."
        char *rom_copy = strdup(rom_str);
        if (!rom_copy) break;
        
        char *pair = strtok_r(rom_copy, ",", &inner_state);
        while (pair) {
            char *equals = strchr(pair, '=');
            if (equals) {
                *equals = '\0';
                char *key = pair;
                char *value = equals + 1;
                
                if (strcmp(key, "file") == 0) {
                    (*roms)[rom_idx].filename = strdup(value);
                } else if (strcmp(key, "extract") == 0) {
                    (*roms)[rom_idx].extract_name = strdup(value);
                } else if (strcmp(key, "type") == 0) {
                    (*roms)[rom_idx].type = strdup(value);
                    (*roms)[rom_idx].type_int = rom_type_from_string(value);
                } else if (strcmp(key, "cs1") == 0) {
                    (*roms)[rom_idx].cs1 = atoi(value);
                } else if (strcmp(key, "cs2") == 0) {
                    if (strcmp(value, "ignore") == 0) {
                        (*roms)[rom_idx].cs2 = 2;  // Use 2 for ignore
                    } else {
                        (*roms)[rom_idx].cs2 = atoi(value);
                    }
                } else if (strcmp(key, "cs3") == 0) {
                    if (strcmp(value, "ignore") == 0) {
                        (*roms)[rom_idx].cs3 = 2;  // Use 2 for ignore
                    } else {
                        (*roms)[rom_idx].cs3 = atoi(value);
                    }
                } else if (strcmp(key, "set") == 0) {
                    (*roms)[rom_idx].set = atoi(value);
                } 
            } else {
                // Standalone flag
                if (strcmp(pair, "dup") == 0) {
                    (*roms)[rom_idx].dup = 1;
                } else if (strcmp(pair, "pad") == 0) {
                    (*roms)[rom_idx].pad = 1;
                }
            }
            pair = strtok_r(NULL, ",", &inner_state);
        }
        
        free(rom_copy);
        rom_idx++;
        rom_str = strtok_r(NULL, " ", &outer_state);
    }
    
    free(work);
    return rom_idx;
}

// Free memory allocated by parse_rom_configs
void free_rom_configs(rom_config_t *roms, int count) {
    if (!roms) return;
    
    for (int i = 0; i < count; i++) {
        free(roms[i].filename);
        if (roms[i].extract_name) {
            free(roms[i].extract_name);
        }
        free(roms[i].type);
    }
    free(roms);
}

static size_t write_callback(void *contents, size_t size, size_t nmemb, download_buffer_t *buffer) {
    size_t realsize = size * nmemb;
    
    if (buffer->size + realsize >= buffer->capacity) {
        size_t new_capacity = buffer->capacity ? buffer->capacity * 2 : 8192;
        while (new_capacity < buffer->size + realsize) new_capacity *= 2;
        
        uint8_t *new_data = realloc(buffer->data, new_capacity);
        if (!new_data) return 0;
        
        buffer->data = new_data;
        buffer->capacity = new_capacity;
    }
    
    memcpy(buffer->data + buffer->size, contents, realsize);
    buffer->size += realsize;
    return realsize;
}

static int load_file_from_url(const char *url, uint8_t **data, size_t *size) {
    CURL *curl = curl_easy_init();
    if (!curl) {
        printf("Error: Failed to initialize libcurl\n");
        return -1;
    }
    
    download_buffer_t buffer = {0};
    
    curl_easy_setopt(curl, CURLOPT_URL, url);
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, write_callback);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &buffer);
    curl_easy_setopt(curl, CURLOPT_FOLLOWLOCATION, 1L);
    
    CURLcode res = curl_easy_perform(curl);
    curl_easy_cleanup(curl);
    
    if (res != CURLE_OK || buffer.size == 0) {
        printf("Error downloading URL: %s\n", curl_easy_strerror(res));
        free(buffer.data);
        return -1;
    }
    
    *data = buffer.data;
    *size = buffer.size;
    return 0;
}

static int load_file_from_file(const char *filename, uint8_t **data, size_t *size) {
    FILE *f = fopen(filename, "rb");
    if (!f) {
        printf("Error: Cannot open ROM file: %s\n", filename);
        return -1;
    }
    
    fseek(f, 0, SEEK_END);
    *size = ftell(f);
    fseek(f, 0, SEEK_SET);
    
    *data = malloc(*size);
    if (!*data) {
        printf("Failed to allocate memory for ROM data: %zu bytes\n", *size);
        fclose(f);
        return -1;
    }
    
    size_t read_bytes = fread(*data, 1, *size, f);
    fclose(f);
    
    if (read_bytes != *size) {
        printf("Error reading ROM file: %s (expected %zu bytes, got %zu)\n", 
               filename, *size, read_bytes);
        free(*data);
        return -1;
    }
    
    return 0;
}

static void url_decode(char *dst, const char *src) {
    char *d = dst;
    const char *s = src;
    
    while (*s) {
        if (*s == '%' && s[1] && s[2]) {
            int hex;
            if (sscanf(s + 1, "%2x", &hex) == 1) {
                *d++ = hex;
                s += 3;
            } else {
                *d++ = *s++;
            }
        } else {
            *d++ = *s++;
        }
    }
    *d = '\0';
}

static int extract_from_zip(uint8_t *zip_data, size_t zip_size, const char *filename, uint8_t **extracted_data, size_t *extracted_size) {
    zip_error_t error;
    zip_source_t *source = zip_source_buffer_create(zip_data, zip_size, 0, &error);
    if (!source) {
        printf("Error: Failed to create zip source buffer\n");
        return -1;
    }
    
    zip_t *archive = zip_open_from_source(source, ZIP_RDONLY, &error);
    if (!archive) {
        printf("Error: Failed to open zip archive: %s\n", zip_error_strerror(&error));
        zip_source_free(source);
        return -1;
    }
    
    // URL decode the filename
    char decoded_filename[strlen(filename) + 1];
    url_decode(decoded_filename, filename);

    zip_file_t *file = zip_fopen(archive, decoded_filename, 0);
    if (!file) {
        printf("Error: Failed to open file '%s' in zip archive: %s\n", filename, zip_strerror(archive));
        
        // List available files for debugging
        printf("Available files in zip:\n");
        zip_int64_t num_entries = zip_get_num_entries(archive, 0);
        for (zip_int64_t i = 0; i < num_entries; i++) {
            const char *name = zip_get_name(archive, i, 0);
            if (name) printf("  %s\n", name);
        }
        
        zip_close(archive);
        return -1;
    }
    
    zip_stat_t stat;
    if (zip_stat(archive, decoded_filename, 0, &stat) != 0) {
        printf("Error: Failed to get file stats for '%s' (decoded from '%s'): %s\n", 
            decoded_filename, filename, zip_strerror(archive));
        zip_fclose(file);
        zip_close(archive);
        return -1;
    }
    
    *extracted_data = malloc(stat.size);
    if (!*extracted_data) {
        printf("Error: Failed to allocate %llu bytes for extracted file\n", (unsigned long long)stat.size);
        zip_fclose(file);
        zip_close(archive);
        return -1;
    }
    
    zip_int64_t bytes_read = zip_fread(file, *extracted_data, stat.size);
    if (bytes_read < 0 || (zip_uint64_t)bytes_read != stat.size) {
        printf("Error: Failed to read file data (read %lld, expected %llu bytes)\n", 
               (long long)bytes_read, (unsigned long long)stat.size);
        free(*extracted_data);
        zip_fclose(file);
        zip_close(archive);
        return -1;
    }
    
    *extracted_size = stat.size;
    printf("Successfully extracted '%s' (%llu bytes) from zip\n", decoded_filename, (unsigned long long)stat.size);
    
    zip_fclose(file);
    zip_close(archive);
    return 0;
}

int load_rom_file(rom_config_t *config, uint8_t **data, size_t *size) {
    int rc;

    if (strstr(config->filename, "http://") == config->filename || strstr(config->filename, "https://") == config->filename) {
        rc = load_file_from_url(config->filename, data, size);
    } else {
        rc = load_file_from_file(config->filename, data, size);
    }
    if (rc == -1) {
        printf("Error loading ROM file: %s\n", config->filename);
        return -1;
    }

    // If extract_name is specified, treat as zip and extract the file
    if (config->extract_name) {
        uint8_t *extracted_data;
        size_t extracted_size;
        
        if (extract_from_zip(*data, *size, config->extract_name, &extracted_data, &extracted_size) == 0) {
            free(*data);  // Free the zip data
            *data = extracted_data;
            *size = extracted_size;
        } else {
            free(*data);
            return -1;
        }
    }

    return 0;
}

// Load all ROM files from configs
int load_all_roms(rom_config_t *configs, int count, loaded_rom_t **loaded_roms) {
    *loaded_roms = calloc(count, sizeof(loaded_rom_t));
    if (!*loaded_roms) return -1;
    
    for (int i = 0; i < count; i++) {
        const char *filename = configs[i].filename;
        
        (*loaded_roms)[i].config = configs[i];
        if (load_rom_file(&configs[i], &(*loaded_roms)[i].data, &(*loaded_roms)[i].size) != 0) {
            // Clean up on failure
            for (int j = 0; j < i; j++) {
                free((*loaded_roms)[j].data);
            }
            free(*loaded_roms);
            return -1;
        }

        int rom_type = rom_type_from_string(configs[i].type);
        size_t expected_size = get_expected_rom_size(rom_type);

        if ((*loaded_roms)[i].size < expected_size) {
            if (configs[i].dup) {
                // Duplicate the ROM data until we get to the correct size
                size_t new_size = expected_size;
                uint8_t *new_data = malloc(new_size);
                if (!new_data) {
                    printf("Error: Failed to allocate memory for duplicated ROM data\n");
                    for (int j = 0; j < i; j++) {
                        free((*loaded_roms)[j].data);
                    }
                    free(*loaded_roms);
                    return -1;
                }
                size_t copy_size = (*loaded_roms)[i].size;
                for (size_t j = 0; j < new_size; j++) {
                    new_data[j] = (*loaded_roms)[i].data[j % copy_size];
                }
                free((*loaded_roms)[i].data);
                (*loaded_roms)[i].data = new_data;
                (*loaded_roms)[i].size = new_size;
            } else if (configs[i].pad) {
                // Pad the rest of the ROM with 0xAA
                size_t new_size = expected_size;
                uint8_t *new_data = malloc(new_size);
                if (!new_data) {
                    printf("Error: Failed to allocate memory for padded ROM data\n");
                    for (int j = 0; j < i; j++) {
                        free((*loaded_roms)[j].data);
                    }
                    free(*loaded_roms);
                    return -1;
                }
                size_t copy_size = (*loaded_roms)[i].size;
                for (size_t j = 0; j < new_size; j++) {
                    if (j < copy_size) {
                        new_data[j] = (*loaded_roms)[i].data[j];
                    } else {
                        new_data[j] = 0xAA; // Pad with 0xAA
                    }
                }
                free((*loaded_roms)[i].data);
                (*loaded_roms)[i].data = new_data;
                (*loaded_roms)[i].size = new_size;
            }
        }
        
        printf("Loaded ROM %d: %s (%zu bytes, type %s)\n", 
               i, filename, (*loaded_roms)[i].size, configs[i].type);
    }
    
    return 0;
}

// Free memory allocated by load_all_roms
void free_all_roms(loaded_rom_t *loaded_roms, int count) {
    if (!loaded_roms) return;
    
    // Free each ROM's data buffer
    for (int i = 0; i < count; i++) {
        free(loaded_roms[i].data);
    }
    
    // Free the array itself
    free(loaded_roms);
}

void print_loaded_rom_analysis(loaded_rom_t *loaded_roms, rom_config_t *configs, int count) {
    printf("\n=== Loaded ROM Analysis ===\n");
    printf("Total ROMs loaded: %d\n", count);
    
    // Print details for each loaded ROM
    for (int i = 0; i < count; i++) {
        printf("\nROM %d:\n", i);
        printf("  File: %s\n", configs[i].filename);
        printf("  Type: %s\n", configs[i].type);
        printf("  Size: %zu bytes\n", loaded_roms[i].size);
        printf("  CS1: %d", configs[i].cs1);
        if (configs[i].cs2 != -1) {
            if (configs[i].cs2 == 2) {
                printf(", CS2: ignore");
            } else {
                printf(", CS2: %d", configs[i].cs2);
            }
        }
        if (configs[i].cs3 != -1) {
            if (configs[i].cs3 == 2) {
                printf(", CS3: ignore");
            } else {
                printf(", CS3: %d", configs[i].cs3);
            }
        }
        if (configs[i].set != -1) {
            printf(", Set: %d", configs[i].set);
        }
        printf("\n");
        
        // Show first few bytes of ROM data
        printf("  First 8 bytes: ");
        for (size_t j = 0; j < 8 && j < loaded_roms[i].size; j++) {
            printf("0x%02X ", loaded_roms[i].data[j]);
        }
        printf("\n");
        
        // Check expected ROM size for type
        int rom_type = rom_type_from_string(configs[i].type);
        size_t expected_size = get_expected_rom_size(rom_type);
        
        if (expected_size > 0) {
            if (loaded_roms[i].size == expected_size) {
                printf("  Size check: OK ✓\n");
            } else {
                printf("  Size check: FAIL (expected %zu bytes) ✗\n", expected_size);
            }
        }
    }
    
    // Compare with generated ROM sets
    printf("\n=== Generated ROM Sets Comparison ===\n");
    printf("Generated sets: %d\n", sdrr_rom_set_count);
    printf("Generated images: %d\n", SDRR_NUM_IMAGES);
    
    if (count == SDRR_NUM_IMAGES) {
        printf("ROM count matches ✓\n");
    } else {
        printf("ROM count mismatch: loaded %d, generated %d ✗\n", count, SDRR_NUM_IMAGES);
    }
    
    // Show generated ROM set info
    for (int i = 0; i < sdrr_rom_set_count; i++) {
        printf("Set %d: %u bytes, %d ROMs\n", i, rom_set[i].size, rom_set[i].rom_count);
    }
}
