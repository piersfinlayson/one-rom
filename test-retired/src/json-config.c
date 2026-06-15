// Functions for JSON hardware configuration

// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <ctype.h>
#include <json-c/json.h>
#include <json-config.h>

#define HW_CONFIG_DIR  "rust/config/json"
static const char* base_dirs[] = {HW_CONFIG_DIR "/", "../" HW_CONFIG_DIR "/"};
static const char* sub_dirs[] = {"third-party/", "user/"};

static char* normalize_filename(const char* hw_rev) {
    size_t len = strlen(hw_rev);
    char* normalized = malloc(len + 6);

    if (!strcmp(hw_rev, "d")) {
        strcpy(normalized, "ice-24-d.json");
        return normalized;
    } else if (!strcmp(hw_rev, "e")) {
        strcpy(normalized, "ice-24-e.json");
        return normalized;
    } else if (!strcmp(hw_rev, "f")) {
        strcpy(normalized, "ice-24-f.json");
        return normalized;
    }
    
    for (size_t i = 0; i < len; i++) {
        if (hw_rev[i] == '_') {
            normalized[i] = '-';
        } else {
            normalized[i] = tolower(hw_rev[i]);
        }
    }
    strcpy(normalized + len, ".json");
    return normalized;
}

static sdrr_mcu_port_t parse_port(const char* port_str) {
    if (!port_str) return PORT_NONE;
    switch (port_str[0]) {
        case 'A': return PORT_A;
        case 'B': return PORT_B;
        case 'C': return PORT_C;
        case 'D': return PORT_D;
        default: return PORT_NONE;
    }
}

static void parse_int_array(json_object* arr, uint8_t* dest, size_t max_size) {
    size_t len = json_object_array_length(arr);
    for (size_t i = 0; i < len && i < max_size; i++) {
        json_object* item = json_object_array_get_idx(arr, i);
        dest[i] = json_object_get_int(item);
    }
}

static void parse_cs_config(json_object* cs_obj, cs_config_t* cs_config) {
    cs_config->pin_2364 = 255;
    cs_config->pin_2332 = 255;
    cs_config->pin_2316 = 255;
    cs_config->pin_23128 = 255;
    cs_config->pin_23256 = 255;
    cs_config->pin_23512 = 255;
    cs_config->pin_231024 = 255;
    cs_config->pin_2704 = 255;
    cs_config->pin_2708 = 255;
    cs_config->pin_2716 = 255;
    cs_config->pin_2732 = 255;
    cs_config->pin_2764 = 255;
    cs_config->pin_27128 = 255;
    cs_config->pin_27256 = 255;
    cs_config->pin_27512 = 255;
    cs_config->pin_27c010 = 255;
    cs_config->pin_27c020 = 255;
    cs_config->pin_27c040 = 255;
    cs_config->pin_27c301 = 255;
    cs_config->pin_27c080 = 255;
    cs_config->pin_27c400 = 255;
    cs_config->pin_28c16 = 255;
    cs_config->pin_28c64 = 255;
    cs_config->pin_28c256 = 255;
    cs_config->pin_28c512 = 255;
    
    json_object* pin_obj;
    if (json_object_object_get_ex(cs_obj, "2364", &pin_obj)) {
        cs_config->pin_2364 = json_object_get_int(pin_obj);
    }
    if (json_object_object_get_ex(cs_obj, "2332", &pin_obj)) {
        cs_config->pin_2332 = json_object_get_int(pin_obj);
    }
    if (json_object_object_get_ex(cs_obj, "2316", &pin_obj)) {
        cs_config->pin_2316 = json_object_get_int(pin_obj);
    }
    if (json_object_object_get_ex(cs_obj, "23128", &pin_obj)) {
        cs_config->pin_23128 = json_object_get_int(pin_obj);
    }
    if (json_object_object_get_ex(cs_obj, "23256", &pin_obj)) {
        cs_config->pin_23256 = json_object_get_int(pin_obj);
    }
    if (json_object_object_get_ex(cs_obj, "23512", &pin_obj)) {
        cs_config->pin_23512 = json_object_get_int(pin_obj);
    }
    if (json_object_object_get_ex(cs_obj, "231024", &pin_obj)) {
        cs_config->pin_231024 = json_object_get_int(pin_obj);
    }
    if (json_object_object_get_ex(cs_obj, "2704", &pin_obj)) {
        cs_config->pin_2704 = json_object_get_int(pin_obj);
    }
    if (json_object_object_get_ex(cs_obj, "2708", &pin_obj)) {
        cs_config->pin_2708 = json_object_get_int(pin_obj);
    }
    if (json_object_object_get_ex(cs_obj, "2716", &pin_obj)) {
        cs_config->pin_2716 = json_object_get_int(pin_obj);
    }
    if (json_object_object_get_ex(cs_obj, "2732", &pin_obj)) {
        cs_config->pin_2732 = json_object_get_int(pin_obj);
    }
    if (json_object_object_get_ex(cs_obj, "2764", &pin_obj)) {
        cs_config->pin_2764 = json_object_get_int(pin_obj);
    }
    if (json_object_object_get_ex(cs_obj, "27128", &pin_obj)) {
        cs_config->pin_27128 = json_object_get_int(pin_obj);
    }
    if (json_object_object_get_ex(cs_obj, "27256", &pin_obj)) {
        cs_config->pin_27256 = json_object_get_int(pin_obj);
    }
    if (json_object_object_get_ex(cs_obj, "27512", &pin_obj)) {
        cs_config->pin_27512 = json_object_get_int(pin_obj);
    }
    if (json_object_object_get_ex(cs_obj, "27c010", &pin_obj)) {
        cs_config->pin_27c010 = json_object_get_int(pin_obj);
    }
    if (json_object_object_get_ex(cs_obj, "27c020", &pin_obj)) {
        cs_config->pin_27c020 = json_object_get_int(pin_obj);
    }
    if (json_object_object_get_ex(cs_obj, "27c040", &pin_obj)) {
        cs_config->pin_27c040 = json_object_get_int(pin_obj);
    }
    if (json_object_object_get_ex(cs_obj, "27c080", &pin_obj)) {
        cs_config->pin_27c080 = json_object_get_int(pin_obj);
    }
    if (json_object_object_get_ex(cs_obj, "27c301", &pin_obj)) {
        cs_config->pin_27c301 = json_object_get_int(pin_obj);
    }
    if (json_object_object_get_ex(cs_obj, "27c400", &pin_obj)) {
        cs_config->pin_27c400 = json_object_get_int(pin_obj);
    }
    if (json_object_object_get_ex(cs_obj, "28c16", &pin_obj)) {
        cs_config->pin_28c16 = json_object_get_int(pin_obj);
    }
    if (json_object_object_get_ex(cs_obj, "28c64", &pin_obj)) {
        cs_config->pin_28c64 = json_object_get_int(pin_obj);
    }
    if (json_object_object_get_ex(cs_obj, "28c256", &pin_obj)) {
        cs_config->pin_28c256 = json_object_get_int(pin_obj);
    }
    if (json_object_object_get_ex(cs_obj, "28c512", &pin_obj)) {
        cs_config->pin_28c512 = json_object_get_int(pin_obj);
    }
}

static FILE* find_config_file(const char* filename) {
    char filepath[512];
    FILE* fp;
    
    // First try base directories
    snprintf(filepath, sizeof(filepath), HW_CONFIG_DIR "/%s", filename);
    fp = fopen(filepath, "r");
    if (fp) return fp;

    snprintf(filepath, sizeof(filepath), "../" HW_CONFIG_DIR "/%s", filename);
    fp = fopen(filepath, "r");
    if (fp) return fp;
    
    // If not found, try subdirectories under whichever base exists    
    for (size_t i = 0; i < 2; i++) {
        for (size_t j = 0; j < 2; j++) {
            snprintf(filepath, sizeof(filepath), "%s%s%s", base_dirs[i], sub_dirs[j], filename);
            fp = fopen(filepath, "r");
            if (fp) return fp;
        }
    }
    
    return NULL;
}

json_config_t* load_json_config(const char* hw_rev) {
    if (!hw_rev) {
        printf("No HW_REV supplied\n");
        return NULL;
    }
    
    char* filename = normalize_filename(hw_rev);
    FILE* fp = find_config_file(filename);
    
    if (!fp) {
        printf("Couldn't find hardware configuration file %s\n", filename);
        free(filename);
        return NULL;
    }
    free(filename);

    fseek(fp, 0, SEEK_END);
    long len = ftell(fp);
    fseek(fp, 0, SEEK_SET);
    
    char* data = malloc(len + 1);
    fread(data, 1, len, fp);
    data[len] = '\0';
    fclose(fp);
    
    json_object* root = json_tokener_parse(data);
    free(data);
    if (!root) {
        printf("Failed to parse JSON configuration\n");
        return NULL;
    }

    json_config_t* config = calloc(1, sizeof(json_config_t));
    
    json_object* desc_obj;
    if (json_object_object_get_ex(root, "description", &desc_obj)) {
        config->description = strdup(json_object_get_string(desc_obj));
    } else {
        printf("Failed to parse JSON configuration - no description entry\n");
        return NULL;
    }
    
    char *error = NULL;

    json_object* rom_obj;
    if (json_object_object_get_ex(root, "chip", &rom_obj)) {
        json_object* pins_obj;
        if (json_object_object_get_ex(rom_obj, "pins", &pins_obj)) {
            json_object* quantity_obj;
            if (json_object_object_get_ex(pins_obj, "quantity", &quantity_obj)) {
                config->rom.pin_count = json_object_get_int(quantity_obj);
            } else {
                error = "quantity";
            }
        } else {
            error = "pins";
        }
    }
    
    json_object* mcu_obj;
    if (json_object_object_get_ex(root, "mcu", &mcu_obj)) {
        json_object* family_obj;
        if (json_object_object_get_ex(mcu_obj, "family", &family_obj)) {
            strncpy(config->mcu.family, json_object_get_string(family_obj), MAX_FAMILY_LEN - 1);
        } else {
            error = "family";
        }
        
        json_object* ports_obj;
        if (json_object_object_get_ex(mcu_obj, "ports", &ports_obj)) {
            json_object* port_obj;
            if (json_object_object_get_ex(ports_obj, "data_port", &port_obj)) {
                config->mcu.ports.data_port = parse_port(json_object_get_string(port_obj));
            } else {
                error = "data_port";
            }
            if (json_object_object_get_ex(ports_obj, "addr_port", &port_obj)) {
                config->mcu.ports.addr_port = parse_port(json_object_get_string(port_obj));
            } else {
                error = "addr_port";
            }
            if (json_object_object_get_ex(ports_obj, "cs_port", &port_obj)) {
                config->mcu.ports.cs_port = parse_port(json_object_get_string(port_obj));
            } else {
                error = "cs_port";
            }
            if (json_object_object_get_ex(ports_obj, "sel_port", &port_obj)) {
                config->mcu.ports.sel_port = parse_port(json_object_get_string(port_obj));
            } else {
                error = "sel_port";
            }
            if (json_object_object_get_ex(ports_obj, "status_port", &port_obj)) {
                config->mcu.ports.status_port = parse_port(json_object_get_string(port_obj));
            } else {
                error = "status_port";
            }
        } else {
            error = "ports";
        }
        
        json_object* pins_obj;
        if (json_object_object_get_ex(mcu_obj, "pins", &pins_obj)) {
            json_object* arr_obj;
            memset(config->mcu.pins.data, 255, sizeof(config->mcu.pins.data));
            if (json_object_object_get_ex(pins_obj, "data", &arr_obj)) {
                parse_int_array(arr_obj, config->mcu.pins.data, NUM_DATA_LINES);
            } else {
                error = "data";
            }
            memset(config->mcu.pins.addr, 255, sizeof(config->mcu.pins.addr));
            if (json_object_object_get_ex(pins_obj, "addr", &arr_obj)) {
                parse_int_array(arr_obj, config->mcu.pins.addr, MAX_ADDR_LINES);
            } else {
                error = "addr";
            }
            if (json_object_object_get_ex(pins_obj, "sel", &arr_obj)) {
                parse_int_array(arr_obj, config->mcu.pins.sel, 7);
            } else {
                error = "sel";
            }
            
            json_object* cs_obj;
            if (json_object_object_get_ex(pins_obj, "cs1", &cs_obj)) {
                parse_cs_config(cs_obj, &config->mcu.pins.cs1);
            } else {
                if (config->rom.pin_count < 32) {
                    // CS1 is optional for 32>=32 pin ROMs
                    error = "cs1";
                }
            }
            if (json_object_object_get_ex(pins_obj, "cs2", &cs_obj)) {
                parse_cs_config(cs_obj, &config->mcu.pins.cs2);
            } else {
                if (config->rom.pin_count < 32) {
                    // CS2 is optional for >=32 pin ROMs
                    error = "cs2";
                }
            }
            if (json_object_object_get_ex(pins_obj, "cs3", &cs_obj)) {
                parse_cs_config(cs_obj, &config->mcu.pins.cs3);
            } else {
                if (config->rom.pin_count < 32) {
                    // CS3 is optional for >=32 pin ROMs
                    error = "cs3";
                }
            }
            if (json_object_object_get_ex(pins_obj, "ce", &cs_obj)) {
                parse_cs_config(cs_obj, &config->mcu.pins.ce);
            } else {
                error = "ce";
            }
            if (json_object_object_get_ex(pins_obj, "oe", &cs_obj)) {
                parse_cs_config(cs_obj, &config->mcu.pins.oe);
            } else {
                error = "oe";
            }
            if (json_object_object_get_ex(pins_obj, "write", &cs_obj)) {
                parse_cs_config(cs_obj, &config->mcu.pins.we);
            }
            
            json_object* pin_obj;
            if (json_object_object_get_ex(pins_obj, "x1", &pin_obj)) {
                config->mcu.pins.x1 = json_object_get_int(pin_obj);
            } else {
                // X1 is optional
            }
            if (json_object_object_get_ex(pins_obj, "x2", &pin_obj)) {
                config->mcu.pins.x2 = json_object_get_int(pin_obj);
            } else {
                // X2 is optional
            }
            if (json_object_object_get_ex(pins_obj, "status", &pin_obj)) {
                config->mcu.pins.status = json_object_get_int(pin_obj);
            } else {
                error = "status";
            }
            if (json_object_object_get_ex(pins_obj, "sel_jumper_pull", &pin_obj)) {
                config->mcu.pins.sel_jumper_pull = json_object_get_int(pin_obj);
            } else {
                error = "sel_jumper_pull";
            }
            if (json_object_object_get_ex(pins_obj, "x_jumper_pull", &pin_obj)) {
                config->mcu.pins.x_jumper_pull = json_object_get_int(pin_obj);
            } else {
                error = "x_jumper_pull";
            }
        } else {
            error = "pins";
        }
    } else {
        error = "mcu";
    }

    if (error != NULL) {
        printf("Failed to parse JSON configuration - no \"%s\"\n", error);
        free(config);
        return NULL;
    }

    json_object_put(root);
    return config;
}

void free_json_config(json_config_t* config) {
    if (config) {
        free(config->description);
        free(config);
    }
}