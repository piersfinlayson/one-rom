// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Test logging functions

// Include an implementation of the APIO logging function
#define APIO_LOG_IMPL
#define APIO_LOG_ENABLE(fmt, ...) printf(fmt "\n", ##__VA_ARGS__)

#include <stdio.h>
#include <stdarg.h>
#include <unistd.h>
#include <assert.h>
#include <stdlib.h>
#include <apio.h>
#include <epio.h>
#include "test/func.h"

void stub_log_v(const char* msg, va_list args) {
    vprintf(msg, args);
    printf("\n");
}

void stub_log(const char* msg, ...) {
    va_list args;
    va_start(args, msg);
    stub_log_v(msg, args);
    va_end(args);
}

void err_log(const char* msg, ...) {
    printf("ERROR: ");
    va_list args;
    va_start(args, msg);
    stub_log_v(msg, args);
    va_end(args);
}

#define LOG_FILE "/tmp/onerom-firmware-output.txt"
static int saved_stdout;
static FILE *file;

void clear_log_file(void) {
    assert(file == NULL && "clear_log_file called while log file is open");
    file = fopen(LOG_FILE, "w");
    assert(file && "Failed to clear firmware log file");
    fclose(file);
    file = NULL;
}

void redirect_stdout_to_file(void) {
    assert(file == NULL && "redirect_stdout_to_file called while log file is already open");
    TST_DBG("Firmware logging: %s", LOG_FILE);
    saved_stdout = dup(STDOUT_FILENO);
    file = fopen(LOG_FILE, "a");
    assert(file && "Failed to open firmware log file");
    dup2(fileno(file), STDOUT_FILENO);
}

void reset_stdout(void) {
    assert(file != NULL && "reset_stdout called while log file is not open");
    dup2(saved_stdout, STDOUT_FILENO);
    close(saved_stdout);
    fclose(file);
    file = NULL;
}

static uint32_t progress_counter = 0;

void inc_progress(void) {
    progress_counter++;
}

uint32_t get_progress(void) {
    return progress_counter;
}

void reset_progress(void) {
    progress_counter = 0;
}

const char *chip_type[NUM_CHIP_TYPES] = {
    "2316",
    "2332",
    "2364",
    "23128",
    "23256",
    "23512",
    "2704",
    "2708",
    "2716",
    "2732",
    "2764",
    "27128",
    "27256",
    "27512",
    "231024",
    "27C010",
    "27C020",
    "27C040",
    "27C080",
    "27C400",
    "6116",
    "27C301",
    "SystemPlugin",
    "UserPlugin",
    "PioPlugin",
    "SST39SF040",
    "28C16",
    "28C64",
    "28C256",
    "28C512",
    "23QL512",
    "23QL384",
    "23C1001",
    "27C200"
};

const char *transform_to_str[] = {
    "none",
    "duplicate",
    "truncate",
    "pad"
};