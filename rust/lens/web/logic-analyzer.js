// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License
//
// One ROM Lens - Browser-based One ROM logic analyzer and PIO emulator

'use strict';

// =============================================================================
// CONFIGURATION AND CONSTANTS
// =============================================================================

const CONFIG = {
    // Timebase configuration
    CYCLES_PER_PIXEL_MIN: 0.01,      // Minimum cycles per pixel (zoomed in)
    CYCLES_PER_PIXEL_MAX: 1000,      // Maximum cycles per pixel (zoomed out)
    CYCLES_PER_PIXEL_DEFAULT: 1.0,   // Default zoom level
    
    // Slider uses log scale mapping: slider value maps to actual cycles/pixel
    // slider = -200 to +300 maps to 0.01 to 1000 cycles/pixel logarithmically
    SLIDER_MIN: -200,
    SLIDER_MAX: 300,
    
    // Pin drive values
    PIN_DRIVEN_LOW: 0,
    PIN_DRIVEN_HIGH: 1,
    PIN_NOT_DRIVEN: 255,
    
    // Waveform rendering
    TRACE_HEIGHT: 20,                // Height of each signal trace in pixels
    TRACE_SPACING: 4,                // Vertical spacing between traces
    HEX_TRACE_HEIGHT: 24,            // Height of hex summary traces
    FONT_SIZE: 12,                   // Font size for labels and hex values
    LABEL_WIDTH: 60,                 // Width reserved for signal labels

    // Graduation ticks: when zoomed in far enough to distinguish individual
    // cycles, mark each cycle boundary with a short tick off the trace line.
    GRADUATION_MAX_CYCLES_PER_PIXEL: 0.2,  // Only show ticks at/below this zoom
    GRADUATION_TICK: 6,                    // Tick length in pixels
    
    // Execution
    DEFAULT_SPEED: 100,              // Default cycles per frame
    
    // Auto-scroll margin (pixels from right edge where we start scrolling)
    AUTOSCROLL_MARGIN: 100,

    // Cursor readout
    CURSOR_LINE_COLOR: '#ffffff',   // Vertical cursor line
    CURSOR_LINE_DASH: [1, 7],       // Cursor line dash pattern
    CURSOR_BIT_FONT_SIZE: 12,       // Compact per-bit value at each trace's row
    CURSOR_SUMMARY_FONT_SIZE: 13,   // Cursor summary readout (cycle / time / bus)
    CURSOR_VALUE_COLOR: '#000000',  // Background behind cursor readouts
    CURSOR_VALUE_TEXT_COLOR: '#ffffff',  // Cursor summary text
    CURSOR_VALUE_PADDING_X: 2,      // Horizontal padding around cursor readouts
    CURSOR_VALUE_PADDING_Y: 2,      // Vertical padding around cursor readouts
    CURSOR_VALUE_HEIGHT_MULTIPLIER: 1.3,  // Account for font ascenders

    // Time axis
    TIME_AXIS_HEIGHT: 40,           // Height reserved for time axis at bottom
    TIME_AXIS_TICK_HEIGHT: 8,       // Height of major tick marks
    TIME_AXIS_MINOR_TICK_HEIGHT: 4, // Height of minor tick marks
    TIME_AXIS_COLOR: '#888888',     // Color for axis and ticks
    TIME_AXIS_TEXT_COLOR: '#ffffff', // Color for cycle numbers

    // Stepping timers
    STEP_INITIAL_DELAY: 500,        // Delay before starting continuous stepping (ms)
    STEP_INTERVAL: 250,             // Interval between steps when holding (ms)

    // Right hand values
    RIGHT_HAND_VALUE_COLOR: '#000000',  // Color for right-hand values
    RIGHT_HAND_VALUE_TEXT_COLOR: '#00ff00',  // Color for right-hand value text

    MAX_SAMPLES: 1000000,  // Keep last 10M samples (~24MB)
}

// Get CSS color variables
function getCSSColor(varName) {
    return getComputedStyle(document.documentElement).getPropertyValue(varName).trim();
}

// Color cache - populated on first render
const COLORS = {};

// Format number with comma separators
function formatNumber(num) {
    return num.toLocaleString();
}

// Convert byte to ASCII character (printable range 32-126)
function byteToASCII(byte) {
    if (byte >= 32 && byte <= 126) {
        return String.fromCharCode(byte);
    }
    return '□';  // Non-printable
}

// =============================================================================
// WASM MODULE WRAPPER
// =============================================================================

class WASMModule {
    constructor() {
        this.module = null;
        this.ready = false;
        this.epioHandle = null;  // Store the epio_t* pointer
    }
    
    async init() {
        // MODULARIZE build: OneRomLens() is a factory returning a Promise for
        // the instantiated module.
        this.module = await OneRomLens();
        this.ready = true;
    }

    // High-level onerom API
    oneromInit() {
        // Boot the emulator: 1 = PIOs up, 0 = booted but PIOs off, -1 = limp.
        const status = this.module.ccall('onerom_init', 'number', [], []);
        return status === 1;
    }
    
    oneromDrivePins(addr, numAddrBits, cs1, cs2, cs3, x1, x2, ce, oe) {
        this.module.ccall('onerom_drive_pins', null,
            ['number', 'number', 'number', 'number', 'number', 'number', 'number', 'number', 'number'],
            [addr, numAddrBits, cs1, cs2, cs3, x1, x2, ce, oe]);
    }
    
    oneromReleasePins() {
        this.module.ccall('onerom_release_pins', null, [], []);
    }
    
    oneromReadData(dataBits) {
        return this.module.ccall('onerom_read_data', 'number', ['number'], [dataBits]);
    }
    
    // Pin mapping API
    oneromGetAddrPin(bit) {
        return this.module.ccall('onerom_get_addr_pin', 'number', ['number'], [bit]);
    }
    
    oneromGetDataPin(bit) {
        return this.module.ccall('onerom_get_data_pin', 'number', ['number'], [bit]);
    }
    
    oneromGetCS1Pin() {
        return this.module.ccall('onerom_get_cs1_pin', 'number', [], []);
    }
    
    oneromGetCS2Pin() {
        return this.module.ccall('onerom_get_cs2_pin', 'number', [], []);
    }
    
    oneromGetCS3Pin() {
        return this.module.ccall('onerom_get_cs3_pin', 'number', [], []);
    }
    
    oneromGetX1Pin() {
        return this.module.ccall('onerom_get_x1_pin', 'number', [], []);
    }
    
    oneromGetX2Pin() {
        return this.module.ccall('onerom_get_x2_pin', 'number', [], []);
    }

    oneromGetCEPin() {
        return this.module.ccall('onerom_get_ce_pin', 'number', [], []);
    }

    oneromGetOEPin() {
        return this.module.ccall('onerom_get_oe_pin', 'number', [], []);
    }

    oneromGetBytePin() {
        return this.module.ccall('onerom_get_byte_pin', 'number', [], []);
    }
    
    // Stepping / sampling.  State is global in the wasm module, so (unlike the
    // old raw-epio build) no handle is threaded through; method names are kept
    // so the rest of the analyzer is unchanged.
    epioStepCycles(cycles) {
        this.module.ccall('onerom_step', null, ['number'], [cycles]);
    }

    epioGetCycleCount() {
        return BigInt(this.module.ccall('onerom_get_cycle_count', 'number', [], []));
    }

    epioResetCycleCount() {
        this.module.ccall('onerom_reset_cycle_count', null, [], []);
    }

    // SYSCLK frequency in MHz reported by the firmware. A Lens cycle is a PIO
    // cycle clocked from SYSCLK, so nanoseconds = cycles * 1000 / this.
    oneromGetSysclkMhz() {
        return this.module.ccall('onerom_get_sysclk_mhz', 'number', [], []);
    }

    epioReadPinStates() {
        return BigInt(this.module.ccall('onerom_read_pin_states', 'number', [], []));
    }

    epioReadDrivenPins() {
        return BigInt(this.module.ccall('onerom_read_driven_pins', 'number', [], []));
    }

    oneromGetPIODisassembly() {
        // Rust returns a pointer to a NUL-terminated string in wasm memory that
        // stays valid until the next call; read it with UTF8ToString.
        const ptr = this.module.ccall('onerom_get_pio_disassembly', 'number', [], []);
        if (!ptr) return "PIO disassembly not available.";
        return this.module.UTF8ToString(ptr);
    }

    oneromLensGetRomSize() {
        return this.module.ccall('onerom_lens_get_rom_size', 'number', [], []);
    }

    oneromLensGetDataBits() {
        return this.module.ccall('onerom_lens_get_num_data_bits', 'number', [], []);
    }

    oneromLensGetNumAddrBits() {
        return this.module.ccall('onerom_lens_get_num_addr_bits', 'number', [], []);
    }

    oneromLensDriveAddr(addr, cs) {
        const accessWidth = parseInt(document.getElementById('accessWidth').value);
        this.module.ccall('onerom_drive_addr', null, 
            ['number', 'number', 'number'], 
            [addr, cs, accessWidth]);
    }

    epioGetGpioInverted(pin) {
        return this.module.ccall('onerom_get_gpio_input_inverted', 'number', ['number'], [pin]);
    }
}

// =============================================================================
// SAMPLE BUFFER
// =============================================================================

class SampleBuffer {
    constructor() {
        this.samples = [];  // Array of {cycle: BigInt, gpios: BigInt, driven: BigInt}
    }
    
    addSample(cycle, gpios, driven) {
        this.samples.push({cycle, gpios, driven});

        // Keep only the most recent samples
        if (this.samples.length > CONFIG.MAX_SAMPLES * 1.1) {
            // Keep only the most recent MAX_SAMPLES
            this.samples = this.samples.slice(-CONFIG.MAX_SAMPLES);
        }
    }
    
    clear() {
        this.samples = [];
    }
    
    getSamplesInRange(startCycle, endCycle) {
        // Binary search to find start index
        let start = 0;
        let end = this.samples.length;
        
        while (start < end) {
            const mid = Math.floor((start + end) / 2);
            if (this.samples[mid].cycle < startCycle) {
                start = mid + 1;
            } else {
                end = mid;
            }
        }
        
        // Collect samples in range
        const result = [];
        for (let i = start; i < this.samples.length; i++) {
            if (this.samples[i].cycle >= endCycle) break;
            result.push(this.samples[i]);
        }
        
        return result;
    }
    
    get length() {
        return this.samples.length;
    }
    
    get lastCycle() {
        return this.samples.length > 0 ? 
            this.samples[this.samples.length - 1].cycle : 0n;
    }
}

// =============================================================================
// SIGNAL DECODER
// =============================================================================

class SignalDecoder {
    constructor(wasmModule) {
        this.wasm = wasmModule;
        this.pinMap = null;
    }
    
    // Build pin mapping from WASM
    buildPinMap(numAddrBits, numDataBits) {
        this.pinMap = {
            addr: [],
            data: [],
            control: {}
        };
        
        for (let i = 0; i < numAddrBits; i++) {
            const pin = this.wasm.oneromGetAddrPin(i);
            this.pinMap.addr[i] = {
                pin: pin,
                inverted: pin !== 255 ? this.wasm.epioGetGpioInverted(pin) : false
            };
        }
        
        for (let i = 0; i < numDataBits; i++) {
            const pin = this.wasm.oneromGetDataPin(i);
            this.pinMap.data[i] = {
                pin: pin,
                inverted: pin !== 255 ? this.wasm.epioGetGpioInverted(pin) : false
            };
        }
        
        const controlMethods = {
            cs1: 'oneromGetCS1Pin',
            cs2: 'oneromGetCS2Pin',
            cs3: 'oneromGetCS3Pin',
            x1: 'oneromGetX1Pin',
            x2: 'oneromGetX2Pin',
            ce: 'oneromGetCEPin',
            oe: 'oneromGetOEPin',
            byte: 'oneromGetBytePin'
        };

        for (const ctrl in controlMethods) {
            const pin = this.wasm[controlMethods[ctrl]]();
            this.pinMap.control[ctrl] = {
                pin: pin,
                inverted: pin !== 255 ? this.wasm.epioGetGpioInverted(pin) : false
            };
        }
    }
    
    extractBit(gpios, pinInfo) {
        const bit = (gpios & (1n << BigInt(pinInfo.pin))) !== 0n ? 1 : 0;
        return pinInfo.inverted ? (bit ^ 1) : bit;  // XOR with 1 to invert if needed
    }
    
    // Decode address value from GPIO state
    decodeAddress(gpios) {
        let addr = 0;
        for (let i = 0; i < this.pinMap.addr.length; i++) {
            if (this.extractBit(gpios, this.pinMap.addr[i])) {
                addr |= (1 << i);
            }
        }
        return addr;
    }
    
    // Decode data value from GPIO state
    decodeData(gpios) {
        let data = 0;
        for (let i = 0; i < this.pinMap.data.length; i++) {
            if (this.extractBit(gpios, this.pinMap.data[i])) {
                data |= (1 << i);
            }
        }
        return data;
    }
    
    // Decode control signal
    decodeControl(gpios, controlName) {
        const pinNum = this.pinMap.control[controlName];
        return this.extractBit(gpios, pinNum);
    }

    // Check if pin is actively driven
    isPinDriven(driven, pinInfo) {
        return (driven & (1n << BigInt(pinInfo.pin))) !== 0n;
    }
}

// =============================================================================
// EXECUTION ENGINE
// =============================================================================

class ExecutionEngine {
    constructor(wasmModule, sampleBuffer) {
        this.wasm = wasmModule;
        this.samples = sampleBuffer;
        
        // Execution state
        this.running = false;
        this.paused = false;
        this.animationId = null;
        
        // Read sequence state machine
        this.currentAddr = 0;
        this.romSize = this.wasm.oneromLensGetRomSize();
        this.maxAddr = 0;
        this.readState = 'idle';
        this.cyclesRemaining = 0;
        
        // Store only what doesn't change during execution
        this.numAddrBits = 13;
        this.cyclesPerFrame = CONFIG.DEFAULT_SPEED;

        // Read mode
        this.direction = 1;  // 1 = incrementing, -1 = decrementing (for there_and_back)
    }
    

    // Start a complete read sequence
    startCompleteRead(numAddrBits, autoStart = true) {
        if (this.running) return;
        
        this.numAddrBits = numAddrBits;
        this.currentAddr = 0;
        this.maxAddr = this.romSize - 1;
        this.direction = 1;
        this.readState = 'drive';
        this.cyclesRemaining = 0;
        
        this.running = true;
        this.paused = !autoStart;  // Start paused if not auto-starting
        
        if (autoStart) {
            this.animate();
        }
    }
    
    pause() {
        this.paused = true;
    }
    
    resume() {
        if (!this.running) return;
        this.paused = false;
        this.animate();
    }
    
    stop() {
        this.running = false;
        this.paused = false;
        this.readState = 'idle';
        if (this.animationId) {
            cancelAnimationFrame(this.animationId);
            this.animationId = null;
        }
    }
    
    // Execute a single cycle and pause
    singleStep() {
        if (!this.running) {
            // Not started yet - need to initialize
            return false;
        }
        
        this.stepOneCycle();
        this.paused = true;
        return true;
    }

    // Animation loop
    animate() {
        if (!this.running || this.paused) return;
        
        // Execute cycles for this frame
        for (let i = 0; i < this.cyclesPerFrame; i++) {
            if (!this.stepOneCycle()) {
                // Sequence complete
                this.stop();
                return;
            }
        }
        
        // Continue animation
        this.animationId = requestAnimationFrame(() => this.animate());
    }
    
    // Step one PIO cycle and update state machine
    stepOneCycle() {
        if (this.currentAddr > this.maxAddr) {
            return false;  // Complete
        }
        
        // Read current values from UI
        const setupCycles = parseInt(document.getElementById('setupCycles').value);
        const recoveryCycles = parseInt(document.getElementById('recoveryCycles').value);
        
        switch (this.readState) {
            case 'drive':
                this.wasm.oneromLensDriveAddr(
                    this.currentAddr,
                    1
                )
                this.readState = 'setup';
                this.cyclesRemaining = setupCycles;
                break;
                
            case 'setup':
                if (--this.cyclesRemaining === 0) {
                    this.wasm.oneromReadData(8);
                    this.readState = 'release';
                }
                break;
                
            case 'release':
                this.wasm.oneromReleasePins();
                this.readState = 'recovery';
                this.cyclesRemaining = recoveryCycles;
                break;
                
            case 'recovery':
                if (--this.cyclesRemaining === 0) {
                    // Read current mode from UI
                    const readMode = document.getElementById('readMode').value;
                    
                    // Calculate next address based on mode
                    switch (readMode) {
                        case 'sequential_once':
                            this.currentAddr++;
                            if (this.currentAddr > this.maxAddr) {
                                return false;  // Complete
                            }
                            break;
                            
                        case 'sequential_forever':
                            this.currentAddr++;
                            if (this.currentAddr > this.maxAddr) {
                                this.currentAddr = 0;  // Wrap around
                            }
                            break;
                            
                        case 'there_and_back':
                            this.currentAddr += this.direction;
                            if (this.currentAddr >= this.maxAddr) {
                                this.direction = -1;  // Start going down
                            } else if (this.currentAddr <= 0) {
                                return false;  // Complete (back at start)
                            }
                            break;
                            
                        case 'random_forever':
                            this.currentAddr = Math.floor(Math.random() * (this.maxAddr + 1));
                            break;
                    }
                    
                    this.readState = 'drive';
                }
                break;
        }
        
        // Step PIO and capture sample
        this.wasm.epioStepCycles(1);
        const cycle = this.wasm.epioGetCycleCount();
        const gpios = this.wasm.epioReadPinStates();
        const driven = this.wasm.epioReadDrivenPins();
        this.samples.addSample(cycle, gpios, driven);
        
        return true;
    }
    
    // Get progress percentage
    getProgress(readMode) {
        if (this.maxAddr === 0) return 0;
        
        if (readMode === 'there_and_back') {
            // First half: 0→maxAddr = 0-50%, second half: maxAddr→0 = 50-100%
            if (this.direction === 1) {
                // Going up
                return Math.floor(50 * this.currentAddr / this.maxAddr);
            } else {
                // Going down
                return Math.floor(50 + 50 * (this.maxAddr - this.currentAddr) / this.maxAddr);
            }
        }
        
        // Sequential modes
        return Math.floor(100 * this.currentAddr / (this.maxAddr + 1));
    }
    
    isRunning() {
        return this.running;
    }
    
    isPaused() {
        return this.paused;
    }
}

// =============================================================================
// WAVEFORM RENDERER
// =============================================================================

class WaveformRenderer {
    constructor(canvas) {
        this.canvas = canvas;
        this.ctx = canvas.getContext('2d');
        this.cyclesPerPixel = CONFIG.CYCLES_PER_PIXEL_DEFAULT;
        this.scrollPos = 0;  // In cycles
        this.autoScroll = true;
        this.sysclkMhz = 0;  // Firmware SYSCLK in MHz; 0 = unknown (cycles only)
        
        // Signal group visibility
        this.showAddr = true;
        this.showData = true;
        this.showCS1 = true;
        this.showCS2 = true;
        this.showCS3 = true;
        this.showX1 = true;
        this.showX2 = true;
        this.showCE = true;
        this.showOE = true;
        this.showByte = true;
        
        // Signal group expansion state
        this.addrExpanded = true;
        this.dataExpanded = true;
        
        this.cursorX = null;  // Mouse X position for cursor
        this.cursorY = null;  // Mouse Y position (summary readout follows it)
        this.cursorCycle = null;  // Cycle at cursor position
        this.currentLayout = [];  // Calculated layout of traces for current decoder

        // Load colors from CSS
        this.loadColors();
    }
    
    loadColors() {
        COLORS.background = getCSSColor('--color-background');
        COLORS.high = getCSSColor('--color-high');
        COLORS.low = getCSSColor('--color-low');
        COLORS.smearedHatch = getCSSColor('--color-smeared-hatch');
        COLORS.duration = getCSSColor('--color-duration');
        COLORS.hex = getCSSColor('--color-hex');
        COLORS.label = getCSSColor('--color-label');
        COLORS.groupBg = getCSSColor('--color-group-bg');
    }
    
    resize() {
        const container = this.canvas.parentElement;
        this.canvas.width = container.clientWidth;
        this.canvas.height = container.clientHeight;
    }
    
    clear() {
        this.ctx.fillStyle = COLORS.background;
        this.ctx.fillRect(0, 0, this.canvas.width, this.canvas.height);
    }
    
    // Convert cycle number to pixel X coordinate
    cycleToPixelX(cycle) {
        return CONFIG.LABEL_WIDTH + (Number(cycle) / this.cyclesPerPixel) - this.scrollPos;
    }
    
    // Update scroll position for auto-scroll
    updateAutoScroll(lastCycle) {
        if (!this.autoScroll) return;
        
        const waveformWidth = this.canvas.width - CONFIG.LABEL_WIDTH;
        const lastCyclePixel = Number(lastCycle) / this.cyclesPerPixel;
        const currentViewEnd = this.scrollPos + waveformWidth;
        
        // If we're within margin of right edge, scroll to keep following
        if (lastCyclePixel > currentViewEnd - CONFIG.AUTOSCROLL_MARGIN) {
            this.scrollPos = Math.max(0, lastCyclePixel - waveformWidth + CONFIG.AUTOSCROLL_MARGIN);
        }
    }
    
    // Calculate Y position for each trace
    calculateTraceLayout(decoder) {
        const MIN_TRACE_HEIGHT = 20;
        const MIN_HEX_TRACE_HEIGHT = 24;
        const MIN_TRACE_SPACING = 4;
        
        // First pass - calculate minimum required height
        let minTotalHeight = MIN_TRACE_SPACING;
        const traceList = [];
        
        // Address group
        if (this.showAddr) {
            traceList.push({
                type: 'addr_hex',
                minHeight: MIN_HEX_TRACE_HEIGHT,
                label: `A[${decoder.pinMap.addr.length-1}:0]`,
                gpio: null
            });
            minTotalHeight += MIN_HEX_TRACE_HEIGHT + MIN_TRACE_SPACING;
            
            if (this.addrExpanded) {
                for (let i = decoder.pinMap.addr.length - 1; i >= 0; i--) {
                    traceList.push({
                        type: 'addr_bit',
                        bit: i,
                        minHeight: MIN_TRACE_HEIGHT,
                        label: `A${i}`,
                        gpio: decoder.pinMap.addr[i]
                    });
                    minTotalHeight += MIN_TRACE_HEIGHT + MIN_TRACE_SPACING;
                }
            }
        }
        
        // Data group
        if (this.showData) {
            traceList.push({
                type: 'data_hex',
                minHeight: MIN_HEX_TRACE_HEIGHT,
                label: `D[${decoder.pinMap.data.length-1}:0]`,
                gpio: null
            });
            minTotalHeight += MIN_HEX_TRACE_HEIGHT + MIN_TRACE_SPACING;
            
            if (this.dataExpanded) {
                for (let i = decoder.pinMap.data.length - 1; i >= 0; i--) {
                    traceList.push({
                        type: 'data_bit',
                        bit: i,
                        minHeight: MIN_TRACE_HEIGHT,
                        label: `D${i}`,
                        gpio: decoder.pinMap.data[i]
                    });
                    minTotalHeight += MIN_TRACE_HEIGHT + MIN_TRACE_SPACING;
                }
            }
        }
        
        // Control signals
        const controls = [
            { name: 'cs1', show: this.showCS1, pin: decoder.pinMap.control.cs1 },
            { name: 'cs2', show: this.showCS2, pin: decoder.pinMap.control.cs2 },
            { name: 'cs3', show: this.showCS3, pin: decoder.pinMap.control.cs3 },
            { name: 'x1', show: this.showX1, pin: decoder.pinMap.control.x1 },
            { name: 'x2', show: this.showX2, pin: decoder.pinMap.control.x2 },
            { name: 'ce', show: this.showCE, pin: decoder.pinMap.control.ce },
            { name: 'oe', show: this.showOE, pin: decoder.pinMap.control.oe },
            { name: 'byte', show: this.showByte, pin: decoder.pinMap.control.byte }
        ];

        for (const ctrl of controls) {
            if (ctrl.show) {
                traceList.push({
                    type: 'control',
                    signal: ctrl.name,
                    minHeight: MIN_TRACE_HEIGHT,
                    label: ctrl.name.toUpperCase(),
                    gpio: ctrl.pin
                });
                minTotalHeight += MIN_TRACE_HEIGHT + MIN_TRACE_SPACING;
            }
        }
        
        // Calculate scale factor (at least 1.0, never shrink below minimum)
        const availableHeight = this.canvas.height - CONFIG.TIME_AXIS_HEIGHT;
        const scaleFactor = Math.max(1.0, availableHeight / minTotalHeight);
        
        // Second pass - apply scaling and calculate positions
        const layout = [];
        let y = MIN_TRACE_SPACING * scaleFactor;
        
        for (const trace of traceList) {
            layout.push({
                ...trace,
                y: y,
                height: trace.minHeight * scaleFactor
            });
            y += (trace.minHeight + MIN_TRACE_SPACING) * scaleFactor;
        }
        
        return layout;
    }

    // Render all waveforms
    render(sampleBuffer, decoder) {
        this.clear();
        
        const layout = this.calculateTraceLayout(decoder);
        this.currentLayout = layout;
        
        // Draw labels
        this.ctx.fillStyle = COLORS.label;
        this.ctx.font = `${CONFIG.FONT_SIZE}px monospace`;
        this.ctx.textAlign = 'right';
        this.ctx.textBaseline = 'middle';
        
        for (const trace of layout) {
            this.ctx.fillText(trace.label, CONFIG.LABEL_WIDTH - 5, trace.y + trace.height / 2);
        }
        
        // Calculate visible cycle range
        const waveformWidth = this.canvas.width - CONFIG.LABEL_WIDTH;
        const startCycle = BigInt(Math.floor(this.scrollPos * this.cyclesPerPixel));
        const endCycle = BigInt(Math.floor((this.scrollPos + waveformWidth) * this.cyclesPerPixel));
        
        // Get all samples in visible range
        const visibleSamples = sampleBuffer.getSamplesInRange(startCycle, endCycle);
        if (visibleSamples.length === 0) return;
        
        // Render each trace
        for (const trace of layout) {
            if (trace.type === 'addr_hex') {
                this.renderHexTrace(trace, visibleSamples, decoder, 'addr', waveformWidth);
            } else if (trace.type === 'data_hex') {
                this.renderHexTrace(trace, visibleSamples, decoder, 'data', waveformWidth);
            } else if (trace.type === 'addr_bit') {
                const pinNum = decoder.pinMap.addr[trace.bit];
                this.renderBitTrace(trace, visibleSamples, decoder, pinNum, waveformWidth);
            } else if (trace.type === 'data_bit') {
                const pinNum = decoder.pinMap.data[trace.bit];
                this.renderBitTrace(trace, visibleSamples, decoder, pinNum, waveformWidth);
            } else if (trace.type === 'control') {
                const pinNum = decoder.pinMap.control[trace.signal];
                this.renderBitTrace(trace, visibleSamples, decoder, pinNum, waveformWidth);
            }
        }

        // Draw cursor if active
        if (this.cursorX !== null && this.cursorCycle !== null) {
            this.renderCursor(sampleBuffer, decoder, layout);
        }
        
        // Draw current values
        this.renderCurrentValues(sampleBuffer, decoder, layout);

        // Draw time axis
        this.renderTimeAxis(waveformWidth);

        // Draw cursor if active
        if (this.cursorX !== null && this.cursorCycle !== null) {
            this.renderCursor(sampleBuffer, decoder, layout);
        }
    }
    
    // Render a single bit trace - draws horizontal lines and vertical edges
    renderBitTrace(trace, samples, decoder, pinNum, waveformWidth) {
        if (samples.length === 0) return;

        // Skip rendering if pin is not connected (255)
        if (pinNum === 255) return;
        
        const yHigh = trace.y + 3;
        const yLow = trace.y + trace.height - 3;
        const canvasEndX = CONFIG.LABEL_WIDTH + waveformWidth;
        
        let previousState = null;
        let previousX = null;
        let previousDriven = null;
        
        this.ctx.lineWidth = 1;
        
        for (const sample of samples) {
            const currentState = decoder.extractBit(sample.gpios, pinNum);
            const isDriven = decoder.isPinDriven(sample.driven, pinNum);
            const currentX = this.cycleToPixelX(sample.cycle);
            
            // Skip if off-screen to the left
            if (currentX < CONFIG.LABEL_WIDTH) {
                previousState = currentState;
                previousDriven = isDriven;
                previousX = currentX;
                continue;
            }
            
            // Stop if off-screen to the right
            if (currentX > canvasEndX) break;
            
            if (previousState !== null && previousX !== null) {
                const segStartX = Math.max(CONFIG.LABEL_WIDTH, previousX);

                // Horizontal level (driven) or mid-line (High-Z), coloured by
                // level; a graduation tick marks the cycle boundary at its start.
                if (previousDriven) {
                    const y = previousState ? yHigh : yLow;
                    this.ctx.strokeStyle = previousState ? COLORS.high : COLORS.low;
                    this.ctx.beginPath();
                    this.ctx.moveTo(segStartX, y);
                    this.ctx.lineTo(currentX, y);
                    this.ctx.stroke();
                    this.drawGraduationTick(segStartX, y, previousState ? 1 : -1);
                } else {
                    const yMid = trace.y + trace.height / 2;
                    this.ctx.strokeStyle = COLORS.smearedHatch;
                    this.ctx.beginPath();
                    this.ctx.moveTo(segStartX, yMid);
                    this.ctx.lineTo(currentX, yMid);
                    this.ctx.stroke();
                    this.drawGraduationTick(segStartX, yMid, 0);
                }

                // Vertical transition edge on drive/level change - neutral grey
                if (isDriven !== previousDriven || (isDriven && currentState !== previousState)) {
                    const prevY = previousDriven
                        ? (previousState ? yHigh : yLow) : trace.y + trace.height / 2;
                    const currY = isDriven
                        ? (currentState ? yHigh : yLow) : trace.y + trace.height / 2;
                    this.ctx.strokeStyle = COLORS.smearedHatch;
                    this.ctx.beginPath();
                    this.ctx.moveTo(currentX, prevY);
                    this.ctx.lineTo(currentX, currY);
                    this.ctx.stroke();
                }
            }
            
            previousState = currentState;
            previousDriven = isDriven;
            previousX = currentX;
        }
        
        // Draw final horizontal line, but only if last sample is visible
        if (previousState !== null && previousX !== null && previousX < canvasEndX) {
            const lastSampleX = this.cycleToPixelX(samples[samples.length - 1].cycle);
            if (lastSampleX > previousX) {
                if (previousDriven) {
                    const y = previousState ? yHigh : yLow;
                    this.ctx.strokeStyle = previousState ? COLORS.high : COLORS.low;
                    this.ctx.beginPath();
                    this.ctx.moveTo(previousX, y);
                    this.ctx.lineTo(Math.min(lastSampleX, canvasEndX), y);
                    this.ctx.stroke();
                    this.drawGraduationTick(previousX, y, previousState ? 1 : -1);
                } else {
                    const yMid = trace.y + trace.height / 2;
                    this.ctx.strokeStyle = COLORS.smearedHatch;
                    this.ctx.beginPath();
                    this.ctx.moveTo(previousX, yMid);
                    this.ctx.lineTo(Math.min(lastSampleX, canvasEndX), yMid);
                    this.ctx.stroke();
                    this.drawGraduationTick(previousX, yMid, 0);
                }
            }
        }
    }

    // Draw a short cycle-boundary graduation tick at x, off a trace line at y,
    // but only when zoomed in far enough to distinguish cycles. `dir` points the
    // tick into the trace: +1 down (below a HIGH line), -1 up (above a LOW line),
    // 0 centred (High-Z mid-line). Inherits the caller's current strokeStyle.
    drawGraduationTick(x, y, dir) {
        if (this.cyclesPerPixel >= CONFIG.GRADUATION_MAX_CYCLES_PER_PIXEL) return;
        const t = CONFIG.GRADUATION_TICK;
        this.ctx.beginPath();
        if (dir === 0) {
            this.ctx.moveTo(x, y - t / 2);
            this.ctx.lineTo(x, y + t / 2);
        } else {
            this.ctx.moveTo(x, y);
            this.ctx.lineTo(x, y + dir * t);
        }
        this.ctx.stroke();
    }

    // Format a held-value duration for a hex region: cycle count, plus real time
    // in nanoseconds when the firmware clock is known (sysclkMhz > 0).
    formatDuration(cycles) {
        const c = Math.round(cycles);
        if (this.sysclkMhz > 0) {
            return `${c}cy ${Math.round(c * 1000 / this.sysclkMhz)}ns`;
        }
        return `${c}cy`;
    }

    // Draw a stable hex region at x: its value on the upper line and, below it,
    // how long the value is held (see formatDuration). Shared by the mid-stream
    // and final-region paths of renderHexTrace.
    drawHexValue(trace, x, hexStr, durationCycles) {
        this.ctx.font = `${CONFIG.FONT_SIZE}px monospace`;
        this.ctx.textAlign = 'left';
        this.ctx.textBaseline = 'middle';
        this.ctx.fillStyle = COLORS.hex;
        this.ctx.fillText(hexStr, x, trace.y + trace.height / 4);
        this.ctx.fillStyle = COLORS.duration;
        this.ctx.fillText(this.formatDuration(durationCycles), x, trace.y + trace.height * 3 / 4);
    }

    // Render hex summary trace
    renderHexTrace(trace, samples, decoder, type, waveformWidth) {
        if (samples.length === 0) return;
        
        const bits = type === 'addr' ? decoder.pinMap.addr : decoder.pinMap.data;
        const canvasEndX = CONFIG.LABEL_WIDTH + waveformWidth;
        
        const hexStrOf = (value) => '0x' + value.toString(16).toUpperCase().padStart(
            Math.ceil(bits.length / 4), '0');

        // Track value changes. stableStartCycle lets us report the exact held
        // duration in cycles, independent of any left-edge clipping of the region.
        let previousValue = null;
        let previousX = null;
        let stableStartX = null;
        let stableStartCycle = null;
        let stableValue = null;

        for (let i = 0; i < samples.length; i++) {
            const sample = samples[i];
            const currentValue = type === 'addr' ?
                decoder.decodeAddress(sample.gpios) :
                decoder.decodeData(sample.gpios);
            const currentX = this.cycleToPixelX(sample.cycle);

            // Skip if off-screen to the left
            if (currentX < CONFIG.LABEL_WIDTH) {
                previousValue = currentValue;
                previousX = currentX;
                stableValue = currentValue;
                stableStartX = currentX;
                stableStartCycle = sample.cycle;
                continue;
            }

            // Stop if off-screen to the right
            if (currentX > canvasEndX) break;

            if (previousValue !== null) {
                if (currentValue === previousValue) {
                    // Value still stable, continue
                } else {
                    // Value changed - draw the stable region if it was stable
                    if (stableValue !== null && stableStartX !== null) {
                        const regionWidth = currentX - Math.max(CONFIG.LABEL_WIDTH, stableStartX);
                        if (regionWidth > 30) {  // Only draw if wide enough
                            const durationCycles = Number(sample.cycle) - Number(stableStartCycle);
                            this.drawHexValue(trace, Math.max(CONFIG.LABEL_WIDTH + 2, stableStartX),
                                hexStrOf(stableValue), durationCycles);
                        }
                    }

                    // Draw transition marker (vertical line)
                    this.ctx.strokeStyle = COLORS.smearedHatch;
                    this.ctx.lineWidth = 1;
                    this.ctx.beginPath();
                    this.ctx.moveTo(currentX, trace.y + 2);
                    this.ctx.lineTo(currentX, trace.y + trace.height - 2);
                    this.ctx.stroke();

                    // Start new stable region
                    stableStartX = currentX;
                    stableStartCycle = sample.cycle;
                    stableValue = currentValue;
                }
            } else {
                // First sample
                stableStartX = currentX;
                stableStartCycle = sample.cycle;
                stableValue = currentValue;
            }

            previousValue = currentValue;
            previousX = currentX;
        }

        // Draw final stable region
        if (stableValue !== null && stableStartX !== null && stableStartX < canvasEndX) {
            const regionWidth = canvasEndX - Math.max(CONFIG.LABEL_WIDTH, stableStartX);
            if (regionWidth > 30) {
                // Don't draw if too close to the end (right-hand value will show it)
                const lastSample = samples[samples.length - 1];
                const lastSampleX = this.cycleToPixelX(lastSample.cycle);
                const tooCloseToEnd = stableStartX > lastSampleX - 150;  // 150px margin

                if (!tooCloseToEnd) {
                    const durationCycles = Number(lastSample.cycle) - Number(stableStartCycle);
                    this.drawHexValue(trace, Math.max(CONFIG.LABEL_WIDTH + 2, stableStartX),
                        hexStrOf(stableValue), durationCycles);
                }
            }
        }
    }

    // Time at an absolute cycle, using the firmware clock (see sysclkMhz).
    // Adaptive units; empty string when the clock is unknown.
    formatTimeAtCycle(cycle) {
        if (this.sysclkMhz <= 0) return '';
        const ns = Number(cycle) * 1000 / this.sysclkMhz;
        if (ns < 1000) return `${ns.toFixed(0)}ns`;
        if (ns < 1e6) return `${(ns / 1000).toFixed(2)}us`;
        return `${(ns / 1e6).toFixed(3)}ms`;
    }

    renderCursor(sampleBuffer, decoder, layout) {
        if (this.cursorX === null || this.cursorCycle === null) return;

        // Vertical cursor line
        this.ctx.strokeStyle = CONFIG.CURSOR_LINE_COLOR;
        this.ctx.lineWidth = 1;
        this.ctx.setLineDash(CONFIG.CURSOR_LINE_DASH);
        this.ctx.beginPath();
        this.ctx.moveTo(this.cursorX, 0);
        this.ctx.lineTo(this.cursorX, this.canvas.height);
        this.ctx.stroke();
        this.ctx.setLineDash([]);

        // Sample under the cursor
        const samples = sampleBuffer.getSamplesInRange(this.cursorCycle, this.cursorCycle + 1n);
        if (samples.length === 0) return;
        const sample = samples[0];

        // Per-bit values: compact, inline at each trace's own row just right of
        // the cursor line, coloured by level (Z = High-Z). Hex rows are omitted
        // here - the address/data bus is summarised in the readout below.
        this.ctx.font = `${CONFIG.CURSOR_BIT_FONT_SIZE}px monospace`;
        this.ctx.textAlign = 'left';
        this.ctx.textBaseline = 'middle';
        const bitX = this.cursorX + CONFIG.CURSOR_VALUE_PADDING_X + 2;
        for (const trace of layout) {
            let pinNum;
            if (trace.type === 'addr_bit') pinNum = decoder.pinMap.addr[trace.bit];
            else if (trace.type === 'data_bit') pinNum = decoder.pinMap.data[trace.bit];
            else if (trace.type === 'control') pinNum = decoder.pinMap.control[trace.signal];
            else continue;

            const bit = decoder.isPinDriven(sample.driven, pinNum)
                ? decoder.extractBit(sample.gpios, pinNum) : null;
            const text = bit === null ? 'Z' : bit.toString();
            const color = bit === null ? COLORS.smearedHatch : (bit ? COLORS.high : COLORS.low);
            const y = trace.y + trace.height / 2;
            const w = this.ctx.measureText(text).width;

            // Small backing box keeps the value legible over the waveform
            this.ctx.fillStyle = CONFIG.CURSOR_VALUE_COLOR;
            this.ctx.fillRect(bitX - CONFIG.CURSOR_VALUE_PADDING_X,
                y - CONFIG.CURSOR_BIT_FONT_SIZE / 2 - 1,
                w + CONFIG.CURSOR_VALUE_PADDING_X * 2, CONFIG.CURSOR_BIT_FONT_SIZE + 2);
            this.ctx.fillStyle = color;
            this.ctx.fillText(text, bitX, y);
        }

        // Summary readout: cycle, real time, and the address/data bus at this
        // cycle. Placed left of the cursor line (so it clears the inline bit
        // values on the right) and follows the mouse Y, clamped on-screen.
        const addr = decoder.decodeAddress(sample.gpios);
        const data = decoder.decodeData(sample.gpios);
        const addrHex = '0x' + addr.toString(16).toUpperCase().padStart(
            Math.ceil(decoder.pinMap.addr.length / 4), '0');
        const dataHex = '0x' + data.toString(16).toUpperCase().padStart(
            Math.ceil(decoder.pinMap.data.length / 4), '0');
        const time = this.formatTimeAtCycle(this.cursorCycle);
        const lines = [`Cycle ${this.cursorCycle.toString()}`];
        if (time) lines.push(time);
        lines.push(`A=${addrHex}`);
        lines.push(`D=${dataHex} ${byteToASCII(data)}`);

        this.ctx.font = `${CONFIG.CURSOR_SUMMARY_FONT_SIZE}px monospace`;
        this.ctx.textAlign = 'right';
        this.ctx.textBaseline = 'top';
        const lineH = CONFIG.CURSOR_SUMMARY_FONT_SIZE * CONFIG.CURSOR_VALUE_HEIGHT_MULTIPLIER;
        const boxW = Math.max(...lines.map(l => this.ctx.measureText(l).width));
        const boxH = lines.length * lineH;
        const boxRight = this.cursorX - CONFIG.CURSOR_VALUE_PADDING_X - 2;
        // Stack the lines, centred on the mouse Y, clamped on-screen
        let top = (this.cursorY ?? 0) - boxH / 2;
        top = Math.max(0, Math.min(top, this.canvas.height - boxH));

        this.ctx.fillStyle = CONFIG.CURSOR_VALUE_COLOR;
        this.ctx.fillRect(boxRight - boxW - CONFIG.CURSOR_VALUE_PADDING_X, top,
            boxW + CONFIG.CURSOR_VALUE_PADDING_X * 2, boxH + CONFIG.CURSOR_VALUE_PADDING_Y);
        this.ctx.fillStyle = CONFIG.CURSOR_VALUE_TEXT_COLOR;
        lines.forEach((line, i) => {
            this.ctx.fillText(line, boxRight, top + CONFIG.CURSOR_VALUE_PADDING_Y + i * lineH);
        });

        this.ctx.textAlign = 'left';  // restore default
    }

    renderTimeAxis(waveformWidth) {
        const startX = CONFIG.LABEL_WIDTH;
        const y = this.canvas.height - CONFIG.TIME_AXIS_HEIGHT;
        
        // Draw baseline
        this.ctx.strokeStyle = CONFIG.TIME_AXIS_COLOR;
        this.ctx.lineWidth = 1;
        this.ctx.beginPath();
        this.ctx.moveTo(startX, y);
        this.ctx.lineTo(startX + waveformWidth, y);
        this.ctx.stroke();
        
        // Calculate nice tick spacing based on zoom level
        const pixelsPerTick = 100; // Aim for tick every ~100 pixels
        const cyclesPerTick = pixelsPerTick * this.cyclesPerPixel;
        
        // Round to nice number: 1, 2, 5, 10, 20, 50, 100, 200, 500, 1000...
        const magnitude = Math.pow(10, Math.floor(Math.log10(cyclesPerTick)));
        const normalized = cyclesPerTick / magnitude;
        let niceTick;
        if (normalized < 2) niceTick = magnitude;
        else if (normalized < 5) niceTick = 2 * magnitude;
        else niceTick = 5 * magnitude;
        
        // Draw ticks
        const startCycle = Math.floor(this.scrollPos * this.cyclesPerPixel / niceTick) * niceTick;
        const endCycle = (this.scrollPos + waveformWidth) * this.cyclesPerPixel;
        
        this.ctx.fillStyle = CONFIG.TIME_AXIS_TEXT_COLOR;
        this.ctx.font = `${CONFIG.FONT_SIZE}px monospace`;
        this.ctx.textAlign = 'center';
        this.ctx.textBaseline = 'top';
        
        for (let cycle = startCycle; cycle <= endCycle; cycle += niceTick) {
            const x = this.cycleToPixelX(BigInt(Math.floor(cycle)));
            
            if (x < startX || x > startX + waveformWidth) continue;
            
            // Major tick
            this.ctx.beginPath();
            this.ctx.strokeStyle = CONFIG.TIME_AXIS_COLOR;
            this.ctx.moveTo(x, y);
            this.ctx.lineTo(x, y + CONFIG.TIME_AXIS_TICK_HEIGHT);
            this.ctx.stroke();
            
            // Label
            this.ctx.fillText(cycle.toString(), x, y + CONFIG.TIME_AXIS_TICK_HEIGHT + 2);
        }
    }

    renderCurrentValues(sampleBuffer, decoder, layout) {
        if (sampleBuffer.length === 0) return;
        
        const lastSample = sampleBuffer.samples[sampleBuffer.length - 1];
        const lastSampleX = this.cycleToPixelX(lastSample.cycle);
        
        this.ctx.font = `${CONFIG.FONT_SIZE + 2}px monospace`;
        this.ctx.textAlign = 'left';
        this.ctx.textBaseline = 'middle';
        
        for (const trace of layout) {
            let valueText = '';
            
            if (trace.type === 'addr_hex') {
                const addr = decoder.decodeAddress(lastSample.gpios);
                valueText = '0x' + addr.toString(16).toUpperCase().padStart(
                    Math.ceil(decoder.pinMap.addr.length / 4), '0');
            } else if (trace.type === 'data_hex') {
                const data = decoder.decodeData(lastSample.gpios);
                const ascii = byteToASCII(data);
                valueText = '0x' + data.toString(16).toUpperCase().padStart(
                    Math.ceil(decoder.pinMap.data.length / 4), '0') + ` ${ascii}`;
            } else if (trace.type === 'addr_bit' || trace.type === 'data_bit' || trace.type === 'control') {
                let pinNum;
                if (trace.type === 'addr_bit') pinNum = decoder.pinMap.addr[trace.bit];
                else if (trace.type === 'data_bit') pinNum = decoder.pinMap.data[trace.bit];
                else pinNum = decoder.pinMap.control[trace.signal];
                
                const isDriven = decoder.isPinDriven(lastSample.driven, pinNum);
                const val = isDriven ? decoder.extractBit(lastSample.gpios, pinNum).toString() : 'Z';
                valueText = trace.label + ': ' + val;
            }
            
            if (valueText) {
                const valueX = lastSampleX + 10;  // Offset from end of signal
                const textWidth = this.ctx.measureText(valueText).width;
                
                // Background box
                this.ctx.fillStyle = CONFIG.RIGHT_HAND_VALUE_COLOR;
                this.ctx.fillRect(
                    valueX - CONFIG.CURSOR_VALUE_PADDING_X,
                    trace.y + trace.height/2 - CONFIG.FONT_SIZE,
                    textWidth + (CONFIG.CURSOR_VALUE_PADDING_X * 2),
                    CONFIG.FONT_SIZE * 2
                );
                
                // Text
                this.ctx.fillStyle = CONFIG.RIGHT_HAND_VALUE_TEXT_COLOR;
                this.ctx.fillText(valueText, valueX, trace.y + trace.height / 2);
            }
        }
    }
}

// =============================================================================
// ANALYZER CONTROLLER
// =============================================================================

class AnalyzerController {
    constructor() {
        this.wasm = new WASMModule();
        this.samples = new SampleBuffer();
        this.decoder = null;
        this.renderer = null;
        this.execution = null;
        this.stepInterval = null;
        this.renderTimer = null;
    }
    
    async init() {
        // Initialize WASM
        await this.wasm.init();
        
        // Initialize One ROM
        const result = this.wasm.oneromInit();
        if (!result) {
            throw new Error('Failed to initialize One ROM emulator');
        }
        
        // Get and display PIO disassembly
        const disassembly = this.wasm.oneromGetPIODisassembly();
        document.getElementById('pioCode').textContent = disassembly;

        // Set up components
        this.decoder = new SignalDecoder(this.wasm);
        this.execution = new ExecutionEngine(this.wasm, this.samples);

        const canvas = document.getElementById('waveform');
        this.renderer = new WaveformRenderer(canvas);
        this.renderer.resize();

        // Read the firmware's SYSCLK once. A Lens cycle is a PIO cycle clocked
        // from SYSCLK, so the renderer uses this to convert cycle counts to real
        // time. Fixed for a given build, so it never needs re-reading.
        this.renderer.sysclkMhz = this.wasm.oneromGetSysclkMhz();
        document.getElementById('sysclk').textContent =
            this.renderer.sysclkMhz > 0 ? `${this.renderer.sysclkMhz} MHz` : 'unknown';

        // Read initial visibility state from HTML
        this.renderer.showAddr = document.getElementById('toggleAddr').checked;
        this.renderer.showData = document.getElementById('toggleData').checked;
        this.renderer.showCS1 = document.getElementById('toggleCS1').checked;
        this.renderer.showCS2 = document.getElementById('toggleCS2').checked;
        this.renderer.showCS3 = document.getElementById('toggleCS3').checked;
        this.renderer.showX1 = document.getElementById('toggleX1').checked;
        this.renderer.showX2 = document.getElementById('toggleX2').checked;
        this.renderer.showCE = document.getElementById('toggleCE').checked;
        this.renderer.showOE = document.getElementById('toggleOE').checked;
        this.renderer.showOE = document.getElementById('toggleOE').checked;
        this.renderer.showByte = document.getElementById('toggleByte').checked;
        
        // Initial pin map
        const addrBits = parseInt(document.getElementById('addrBits').value);
        const dataBits = parseInt(document.getElementById('dataBits').value);
        this.decoder.buildPinMap(addrBits, dataBits);
        
        // Set up UI event handlers
        this.setupEventHandlers();
        
        // Start render loop
        this.startRenderLoop();

        this.updateExecutionButtons(); 
        this.updateRomSize();
        this.updateStatus('Ready');
    }
    
    setupEventHandlers() {
        // Start button
        document.getElementById('startBtn').addEventListener('click', () => {
            this.startExecution();
        });
        
        // Pause button
        document.getElementById('pauseBtn').addEventListener('click', () => {
            if (this.execution.isPaused()) {
                this.execution.resume();
                this.updateExecutionButtons();
            } else {
                this.execution.pause();
                this.updateExecutionButtons();
            }
        });
        
        // Stop button
        document.getElementById('stopBtn').addEventListener('click', () => {
            this.execution.stop();
            this.updateExecutionButtons();
            this.updateStatus('Stopped');
        });
        
        // Clear button
        document.getElementById('clearBtn').addEventListener('click', () => {
            this.execution.stop();
            this.samples.clear();
            this.wasm.epioResetCycleCount();
            this.renderer.scrollPos = 0;
            this.updateDisplay();
            this.updateExecutionButtons();
            this.updateStatus('Cleared');
        });
        
        // Address bits change
        document.getElementById('addrBits').addEventListener('change', (e) => {
            const addrBits = parseInt(e.target.value);
            const dataBits = parseInt(document.getElementById('dataBits').value);
            this.decoder.buildPinMap(addrBits, dataBits);
            this.updateDisplay();
        });

        // Data bits change
        document.getElementById('dataBits').addEventListener('change', (e) => {
            const dataBits = parseInt(e.target.value);
            const accessWidth = document.getElementById('accessWidth');
            
            if (dataBits === 8) {
                // Force 8-bit access when only 8 data lines
                accessWidth.value = '8';
                accessWidth.disabled = true;
            } else {
                // Enable choice for 16 data lines
                accessWidth.disabled = false;
            }
            
            // Rebuild decoder as before
            const addrBits = parseInt(document.getElementById('addrBits').value);
            this.decoder.buildPinMap(addrBits, dataBits);
            this.updateDisplay();
        });        
        // Speed control
        document.getElementById('speedControl').addEventListener('change', (e) => {
            this.execution.cyclesPerFrame = parseInt(e.target.value);
        });
        
        // Cycles per pixel slider
        const slider = document.getElementById('cyclesPerPixel');
        const display = document.getElementById('cyclesPerPixelValue');
        slider.addEventListener('input', (e) => {
            const sliderValue = parseFloat(e.target.value);
            
            // Calculate new cycles per pixel
            const oldCyclesPerPixel = this.renderer.cyclesPerPixel;
            const newCyclesPerPixel = this.sliderToCyclesPerPixel(sliderValue);
            
            // Adjust scroll position to keep center of view stable
            const canvasWidth = this.renderer.canvas.width - CONFIG.LABEL_WIDTH;
            const centerCycle = (this.renderer.scrollPos + canvasWidth / 2) * oldCyclesPerPixel;
            this.renderer.scrollPos = Math.max(0, (centerCycle / newCyclesPerPixel) - canvasWidth / 2);
            
            this.renderer.cyclesPerPixel = newCyclesPerPixel;
            display.textContent = this.renderer.cyclesPerPixel.toFixed(2);
        });
        
        // Auto-scroll toggle
        document.getElementById('autoScroll').addEventListener('change', (e) => {
            this.renderer.autoScroll = e.target.checked;
        });
        
        // Scrollbar
        const scrollbar = document.getElementById('scrollbar');
        scrollbar.addEventListener('input', (e) => {
            this.renderer.scrollPos = parseFloat(e.target.value);
            
            // If scrolled to near the end (within 0.1% of max), re-enable auto-scroll
            const scrollRange = parseFloat(scrollbar.max) - parseFloat(scrollbar.min);
            const distanceFromEnd = parseFloat(scrollbar.max) - parseFloat(e.target.value);
            
            if (distanceFromEnd < scrollRange * 0.001) {
                // Near the end - re-enable auto-scroll
                this.renderer.autoScroll = true;
                document.getElementById('autoScroll').checked = true;
            } else {
                // Not at end - disable auto-scroll
                this.renderer.autoScroll = false;
                document.getElementById('autoScroll').checked = false;
            }
        });
        
        // Window resize
        window.addEventListener('resize', () => {
            this.renderer.resize();
        });
        
        // Signal group toggles
        document.getElementById('toggleAddr').addEventListener('change', (e) => {
            this.renderer.showAddr = e.target.checked;
        });
        
        document.getElementById('toggleData').addEventListener('change', (e) => {
            this.renderer.showData = e.target.checked;
        });
        
        // Control signal toggles
        document.getElementById('toggleCS1').addEventListener('change', (e) => {
            this.renderer.showCS1 = e.target.checked;
        });

        document.getElementById('toggleCS2').addEventListener('change', (e) => {
            this.renderer.showCS2 = e.target.checked;
        });

        document.getElementById('toggleCS3').addEventListener('change', (e) => {
            this.renderer.showCS3 = e.target.checked;
        });

        document.getElementById('toggleX1').addEventListener('change', (e) => {
            this.renderer.showX1 = e.target.checked;
        });

        document.getElementById('toggleX2').addEventListener('change', (e) => {
            this.renderer.showX2 = e.target.checked;
        });

        document.getElementById('toggleCE').addEventListener('change', (e) => {
            this.renderer.showCE = e.target.checked;
        });

        document.getElementById('toggleOE').addEventListener('change', (e) => {
            this.renderer.showOE = e.target.checked;
        });

        document.getElementById('toggleByte').addEventListener('change', (e) => {
            this.renderer.showByte = e.target.checked;
        });
        
        // Collapse/expand buttons
        document.getElementById('collapseAddr').addEventListener('click', (e) => {
            this.renderer.addrExpanded = !this.renderer.addrExpanded;
            e.target.textContent = this.renderer.addrExpanded ? '−' : '+';
        });
        
        document.getElementById('collapseData').addEventListener('click', (e) => {
            this.renderer.dataExpanded = !this.renderer.dataExpanded;
            e.target.textContent = this.renderer.dataExpanded ? '−' : '+';
        });

        // Tooltip element
        const tooltip = document.getElementById('signalTooltip');

        // Cursor tracking on canvas WITH tooltip support
        this.renderer.canvas.addEventListener('mousemove', (e) => {
            const rect = this.renderer.canvas.getBoundingClientRect();
            const x = e.clientX - rect.left;
            const y = e.clientY - rect.top;
            
            // Tooltip logic for label area
            if (x <= CONFIG.LABEL_WIDTH && this.renderer.currentLayout) {
                let foundTrace = false;
                for (const trace of this.renderer.currentLayout) {
                    if (y >= trace.y && y <= trace.y + trace.height) {
                        if (trace.gpio !== null && trace.gpio.pin !== CONFIG.PIN_NOT_DRIVEN) {
                            tooltip.textContent = `GPIO ${trace.gpio.pin}`;
                            tooltip.style.display = 'block';
                            tooltip.style.left = (e.clientX + 10) + 'px';
                            tooltip.style.top = (rect.top + trace.y + trace.height / 2 - 10) + 'px';  // ← Center on trace
                            foundTrace = true;
                        }
                        break;
                    }
                }
                if (!foundTrace) {
                    tooltip.style.display = 'none';
                }
                // In label area - no cursor
                this.renderer.cursorX = null;
                this.renderer.cursorCycle = null;
            }
            // Only show cursor if over waveform area (not labels)
            else if (x > CONFIG.LABEL_WIDTH) {
                tooltip.style.display = 'none';
                this.renderer.cursorX = x;
                this.renderer.cursorY = y;
                // Calculate which cycle the cursor is over
                const cycleOffset = (x - CONFIG.LABEL_WIDTH + this.renderer.scrollPos) * this.renderer.cyclesPerPixel;
                this.renderer.cursorCycle = BigInt(Math.floor(cycleOffset));
            } else {
                tooltip.style.display = 'none';
                this.renderer.cursorX = null;
                this.renderer.cursorCycle = null;
            }
        });

        this.renderer.canvas.addEventListener('mouseleave', () => {
            tooltip.style.display = 'none';
            this.renderer.cursorX = null;
            this.renderer.cursorCycle = null;
        });

        // Step button - single click or hold to step continuously
        const stepBtn = document.getElementById('stepBtn');
        let stepButtonHeld = false; 

        stepBtn.addEventListener('mousedown', () => {
            stepButtonHeld = true; 

            if (!this.execution.isRunning()) {
                // Starting fresh - clear samples like Start button does
                this.samples.clear();
                this.wasm.epioResetCycleCount();
                this.renderer.scrollPos = 0;
                
                const addrBits = parseInt(document.getElementById('addrBits').value);
                this.execution.startCompleteRead(addrBits, false);  // Initialize but don't animate
            }
            
            // First step immediately
            this.execution.singleStep();
            this.updateExecutionButtons();
            
            // Start continuous stepping after short delay
            setTimeout(() => {
                if (stepButtonHeld && this.stepInterval === null) {
                    this.stepInterval = setInterval(() => {
                        this.execution.singleStep();
                    }, CONFIG.STEP_INTERVAL);
                }
            }, CONFIG.STEP_INITIAL_DELAY);  // 500ms delay before continuous stepping starts
        });

        stepBtn.addEventListener('mouseup', () => {
            stepButtonHeld = false; 
            if (this.stepInterval) {
                clearInterval(this.stepInterval);
                this.stepInterval = null;
            }
        });

        stepBtn.addEventListener('mouseleave', () => {
            stepButtonHeld = false;
            if (this.stepInterval) {
                clearInterval(this.stepInterval);
                this.stepInterval = null;
            }
        });
    }
    
    startExecution() {
        const addrBits = parseInt(document.getElementById('addrBits').value);
        const readMode = document.getElementById('readMode').value;
        
        this.samples.clear();
        this.wasm.epioResetCycleCount();
        this.renderer.scrollPos = 0;
        
        this.execution.startCompleteRead(addrBits);
        this.updateExecutionButtons();
        this.updateStatus('Running');
    }
    
    updateExecutionButtons() {
        const startBtn = document.getElementById('startBtn');
        const stepBtn = document.getElementById('stepBtn');
        const pauseBtn = document.getElementById('pauseBtn');
        const stopBtn = document.getElementById('stopBtn');
        
        if (this.execution.isRunning()) {
            startBtn.disabled = true;
            stepBtn.disabled = false;
            pauseBtn.disabled = false;
            stopBtn.disabled = false;
            pauseBtn.textContent = this.execution.isPaused() ? 'Resume' : 'Pause';
        } else {
            startBtn.disabled = false;
            stepBtn.disabled = false;
            pauseBtn.disabled = true;
            stopBtn.disabled = true;
            pauseBtn.textContent = 'Pause';
        }
    }
    
    // Convert slider value to cycles per pixel (logarithmic scale)
    sliderToCyclesPerPixel(sliderValue) {
        const minLog = Math.log10(CONFIG.CYCLES_PER_PIXEL_MIN);
        const maxLog = Math.log10(CONFIG.CYCLES_PER_PIXEL_MAX);
        const range = maxLog - minLog;
        
        const sliderRange = CONFIG.SLIDER_MAX - CONFIG.SLIDER_MIN;
        const normalized = (sliderValue - CONFIG.SLIDER_MIN) / sliderRange;
        
        const logValue = minLog + normalized * range;
        return Math.pow(10, logValue);
    }
    
    // Continuous render loop
    startRenderLoop() {
        const render = () => {
            if (this.samples.length > 0) {
                this.renderer.updateAutoScroll(this.samples.lastCycle);
            }
            this.updateDisplay();
            
            const readMode = document.getElementById('readMode').value;
            if (this.execution.isRunning()) {
                if (this.execution.isPaused()) {
                    // Check if actively stepping
                    if (this.stepInterval !== null) {
                        this.updateStatus('Stepping');
                    } else {
                        this.updateStatus('Paused');
                    }
                } else {
                    this.updateStatus('Running');
                }
                
                if (readMode === 'sequential_once' || readMode === 'there_and_back') {
                    document.getElementById('progress').textContent = 
                        this.execution.getProgress(readMode) + '%';
                } else {
                    document.getElementById('progress').textContent = '∞';
                }
            } else if (this.samples.length > 0) {
                // Check if we naturally completed (not just stopped)
                const readMode = document.getElementById('readMode').value;
                const isFiniteMode = readMode === 'sequential_once' || readMode === 'there_and_back';
                const progress = this.execution.getProgress(readMode);
                
                if (isFiniteMode && progress >= 99) {
                    this.updateStatus('Complete');
                } else {
                    this.updateStatus('Stopped');
                }
                
                if (isFiniteMode) {
                    document.getElementById('progress').textContent = progress + '%';
                }
                
                this.updateExecutionButtons();
            }
            
            this.renderTimer = requestAnimationFrame(render);
        };
        render();
    }
    
    updateDisplay() {
        this.renderer.render(this.samples, this.decoder);
        this.updateSampleCount();
        this.updateScrollbar();
    }

    updateRomSize() {
        const romSizeBytes = this.wasm.oneromLensGetRomSize();
        document.getElementById('romSize').textContent = formatNumber(romSizeBytes) + ' bytes';
        const dataBits = this.wasm.oneromLensGetDataBits();
        this.setAddressBitsForRom(romSizeBytes);
        this.setDataBitsForRom(dataBits);
    }
    
    updateStatus(status) {
        document.getElementById('status').textContent = status;
    }
    
    updateSampleCount() {
        document.getElementById('sampleCount').textContent = formatNumber(this.samples.length);
        document.getElementById('cycleCount').textContent = formatNumber(Number(this.samples.lastCycle));
    }
    
    updateScrollbar() {
        const scrollbar = document.getElementById('scrollbar');
        const canvasWidth = this.renderer.canvas.width - CONFIG.LABEL_WIDTH;
        const maxScroll = Math.max(0, 
            Number(this.samples.lastCycle) / this.renderer.cyclesPerPixel - canvasWidth);
        
        scrollbar.max = maxScroll;
        if (this.renderer.autoScroll) {
            scrollbar.value = this.renderer.scrollPos;
        }
    }

    setAddressBitsForRom(romSizeBytes) {
        const addrBits = this.wasm.oneromLensGetNumAddrBits();
        
        // Update dropdown
        const dropdown = document.getElementById('addrBits');
        dropdown.value = addrBits;
        
        // Trigger the change to update decoder
        const dataBits = parseInt(document.getElementById('dataBits').value);
        this.decoder.buildPinMap(addrBits, dataBits);
        this.updateDisplay();
        
        console.log(`ROM size: ${romSizeBytes} bytes → ${addrBits} address bits`);
    }

    setDataBitsForRom(dataBits) {
        // Update dropdown
        const dropdown = document.getElementById('dataBits');
        dropdown.value = dataBits;
        console.log(`ROM data bits: ${dataBits} bits`);

        // Set access width default based on ROM type
        const accessWidth = document.getElementById('accessWidth');
        if (dataBits === 16) {
            accessWidth.value = '16';  // Default to word mode for 16-bit ROMs
            accessWidth.disabled = false;
            
            // 16-bit ROM (27C400) - use BYTE/CE/OE, not CS1
            document.getElementById('toggleCS1').checked = false;
            document.getElementById('toggleByte').checked = true;
            document.getElementById('toggleCE').checked = true;
            document.getElementById('toggleOE').checked = true;
            
            // Update renderer state
            this.renderer.showCS1 = false;
            this.renderer.showByte = true;
            this.renderer.showCE = true;
            this.renderer.showOE = true;
        } else {
            accessWidth.value = '8';
            accessWidth.disabled = true;
            
            // 8-bit ROM - use CS1, hide BYTE
            document.getElementById('toggleCS1').checked = true;
            document.getElementById('toggleByte').checked = false;
            document.getElementById('toggleCE').checked = false;
            document.getElementById('toggleOE').checked = false;
            
            // Update renderer state
            this.renderer.showCS1 = true;
            this.renderer.showByte = false;
            this.renderer.showCE = false;
            this.renderer.showOE = false;
        }

        // Trigger the change to update decoder
        const addrBits = parseInt(document.getElementById('addrBits').value);
        this.decoder.buildPinMap(addrBits, dataBits);
        this.updateDisplay();
        
        console.log(`ROM data bits: ${dataBits} bits`);
    }
}

// =============================================================================
// INITIALIZATION
// =============================================================================

let analyzer;

window.addEventListener('load', async () => {
    try {
        analyzer = new AnalyzerController();
        await analyzer.init();
    } catch (error) {
        console.error('Failed to initialize analyzer:', error);
        document.getElementById('status').textContent = 'Error: ' + error.message;
    }
});