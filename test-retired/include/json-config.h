// Data structures for JSON hardware configuration

// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#ifndef JSON_CONFIG_H
#define JSON_CONFIG_H

#include <stdint.h>
#include "config_base.h"

#define NUM_DATA_LINES 16
#define MAX_ADDR_LINES 32
#define MAX_FAMILY_LEN 16

typedef struct {
    uint8_t pin_count;
} json_rom_config_t;

typedef struct {
    sdrr_mcu_port_t data_port;
    sdrr_mcu_port_t addr_port;
    sdrr_mcu_port_t cs_port;
    sdrr_mcu_port_t sel_port;
    sdrr_mcu_port_t status_port;
} mcu_ports_t;

typedef struct {
    uint8_t pin_2316;
    uint8_t pin_2332;
    uint8_t pin_2364;
    uint8_t pin_23128;
    uint8_t pin_23256;
    uint8_t pin_23512;
    uint8_t pin_231024;
    uint8_t pin_2704;
    uint8_t pin_2708;
    uint8_t pin_2716;
    uint8_t pin_2732;
    uint8_t pin_2764;
    uint8_t pin_27128;
    uint8_t pin_27256;
    uint8_t pin_27512;
    uint8_t pin_27c010;
    uint8_t pin_27c020;
    uint8_t pin_27c040;
    uint8_t pin_27c080;
    uint8_t pin_27c301;
    uint8_t pin_27c400;
    uint8_t pin_28c16;
    uint8_t pin_28c64;
    uint8_t pin_28c256;
    uint8_t pin_28c512;
} cs_config_t;

typedef struct {
    uint8_t data[NUM_DATA_LINES];
    uint8_t addr[MAX_ADDR_LINES];
    cs_config_t cs1;
    cs_config_t cs2;
    cs_config_t cs3;
    cs_config_t ce;
    cs_config_t oe;
    uint8_t x1;
    uint8_t x2;
    uint8_t x_jumper_pull;
    uint8_t sel[7];
    uint8_t sel_jumper_pull;
    uint8_t status;
    cs_config_t we;
} mcu_pins_t;

typedef struct {
    char family[MAX_FAMILY_LEN];
    mcu_ports_t ports;
    mcu_pins_t pins;
} mcu_config_t;

typedef struct {
    char *description;
    json_rom_config_t rom;
    mcu_config_t mcu;
} json_config_t;

#endif // JSON_CONFIG_H
