# Voltage Levels

## Background

One of the biggest difficulties of this project, alongside hitting the timing requirements, was finding a microcontroller tha could support the 5V logic levels required by retro systems.

The STM32F1 series (64-bit variant) on the face of it has the required number of 5V tolerant (FT) GPIOs, but on further examination, there are not sufficient contiguous and starting at pin 0, FT pins on any port to hit the required performance - that is to allow the assembly code to perform optimal chip select comparisons, address lookups, and apply the results those to the data pins.

Therefore the STM32F4 series was initially chosen, followed by the RP2350, both to give sufficient raw horsepower (clock speed and flash instruction prefetch and cache), and also to provide the required FT hardware configuration.

> **The RP2350 ADC pins are the one exception.** Almost every RP2350 GPIO is 5V
> tolerant, but the ADC-capable GPIOs are **not** - they are 3.3V-only and must
> not exceed the 3.3V IO supply. These are GPIO26-29 on the RP2350A (the QFN-60,
> used by the 24/28-pin Fire boards) and GPIO40-47 on the RP2350B (the QFN-80,
> used by the 32/40-pin Fire boards). One ROM never routes a 5V ROM-bus signal to
> one of these pins; where they are used it is for image-select, status-LED or USB
> signals on the top-edge header, which stay within 3.3V. The CLI's
> `onerom board header --board <board>` view flags each such pad as `!!3V3!!` so it is
> obvious which header pads must be kept at or below 3.3V.

There are two areas which are important to understand when considering voltage levels:
1. The logic level compatibility between One ROM and the retro system - that is, ensuring that One ROM's outputs are within the acceptable input levels of the retro system, and vice versa.  See [5V and 3.3V Logic Levels](#5v-and-33v-logic-levels).
2. The absolute maximum voltage levels that One ROM can tolerate on its pins, especially during power-on, when One ROM's MCU VDD is not yet at 3.3V.  See [Absolute Maximum VIN](#absolute-maximum-vin).

## 5V and 3.3V Logic Levels

As One ROM's purpose is to replace 5V logic level ROMs, it must be compatible with the voltage levels required by those systems.  We do this by examining and comparing the [STM32F411 datasheet](https://www.st.com/resource/en/datasheet/stm32f411re.pdf) (as a representative STM32F4 family chip) with the datasheets for the:

- [6502](http://www.6502.org/documents/datasheets/mos/mos_65ce02_mpu.pdf)
- [6567](http://www.6502.org/documents/datasheets/mos/mos_6567_vic_ii_preliminary.pdf) (C64 VIC-II chip)
- [6560/6561](http://www.6502.org/documents/datasheets/mos/mos_6560_6561_vic.pdf) (VIC-20 VIC chip)

(A similar analysis has been done for the RP2350, although at the highest GPIO drive strength, 12mA, the RP2350 strictly contravenes the requires specification.  Therefore One ROM Fire uses 8mA drive strength, which is within specification.)

### One ROM outputs, 6502 inputs

Note - One ROM's 5V outputs are the data lines, PA0-7.  These are set to "fast" speed, with IIO = +8mA.  We avoid "high" speed, as this can result in a maximum low output voltage VOL of 1.3V, which is too high for the 6502's maximum low input voltage VIL of 0.8V.

| One ROM VOL | 6502 VIL | 6567 VIL | 6560/6561 VIL |
|----------|----------|----------|---------------|
| 0.4V     | 0.8V     | 0.8V     | 0.4V          |

One ROM's maxmimum low output voltage is less than the 6502/6567's maximum low input voltage, and the same as the 6560/6561, so we're good.

| One ROM VOH | 6502 VIH | 6567 VIH | 6560/6561 VIH |
|----------|----------|----------|---------------|
| 2.4V*     | 2.0V     | 2.0V     | 2.4V          |

*Note it is highly likely the STM32F4 datasheet says that VOH is VDD-0.4V = 2.9V in this case - so we take 2.4V to be conservative.*

One ROM's maximum minimum output voltage is greater than the 6502/6567's minimum high input voltage, and the same as the 6560/6561, so we're good.

### One ROM inputs, 6502 outputs

| One ROM VIL | 6502 VOL | 6567 VOL | 6560/6561 VOL |
|----------|----------|----------|---------------|
| 1.0V     | 0.4V     | 0.4V     | 0.4V          |

One ROM's minimum low input voltage is greater than the 6502/6567/6560/6561's maximum low output voltage so we're good.

| One ROM VIH | 6502 VOH | 6567 VOH | 6560/6561 VOH |
|----------|----------|----------|---------------|
| 2.3V     | 2.4V     | 2.4V     | 2.4V          |

One ROM's minimum high input voltage is less than the 6502/6567/6560/6561's minimum high output voltage so we're good.

## Absolute Maximum VIN

According to the MCU datasheets, the absolute maximum rated voltage on any 5V tolerant pin is VDD + 4.0V for the STM32F4 and VDD + 3.63V on the RP2350.  There is also a 5.5V absolute maximum - so when VDD is at its usual 3.3V, the absolute maximum on any pin is 5.5V.

When VDD is 0V (before the voltage regulator is powering the chip), this means the absolute maximum voltage on any pin is 4.0V/3.63V (Ice/Fire).  When One ROM is installed in a retro system and powered on there is likely to be a 20us delay (see AP2112K-3.3TRG1 datasheet) between VCC being applied to One ROM and the voltage regulator outputting 3.3V to the MCU.

During this time then, technically, according to the MCUs' datasheets, the voltage on any GPIO must not exceed 4.0V/3.63V.  This is the case for most pins on most retro host systems as most systems have a reset circuit, and the bus master(s) are often held in reset for much longer than 20us - so most pins will be a 0V.  However, in some cases, some CS lines are permanently pulled to 5V, either through pull-ups or directly.  This means that 5.0V is applied to those pins for around 20us before VDD comes up to 3.3V.

System resets (where the internal host's reset line is used to reset the host system) typically do not trigger the 20us over-voltage scenario, as the MCU's VDD remains at already at 3.3V, as 5V remains applied to One ROM's VCC pin.  So, this applies to physical power-on (after power-off) events only.

To provide confidence in One ROM as a solution given its use strictly contravening the MCUs' datasheet figures, validation testing has been done with both Ice and Fire 24 boards, to ensure it is tolerant of 5.0V being applied to a single CS pin through a 0R resistor, across many power on events.

The test philosophy used was as follows:

- Use a 555 timer circuit to drive a transistor to power One ROM on and off - around 2.5s on, 1.5s off for a 4s/0.25Hz cycle.
- Hard pull (through a 0R resistor or wire) one CS pin (One ROM physical pin 20) to the 555/transistor switched VCC - i.e. it is powered at the same time as VCC is provided to One ROMs power pin, 24.
- Assume that a One ROM will receive, in heavy use, an average of 10 power cycles in any day = ~3600 cycles a year.  Hence one hour of testing (900 cycles) is equivalent to around 3 months of heavy use.
- Run the test for at least 24 hours = 6 years equivalent heavy use.
- After the test, validate that the all pins still operate correctly, by ensuring One ROM 24 can still serve a 2364 C64 kernal ROM in a C64.  If any address, data or CS line were damaged, the C64 would fail to boot.

Fire 24 serves as a reasonable proxy for Fire 28, as the designs are essentially identical, with a different GPIO mapping and couple more GPIOs used.

The 2.5s/1.5s timing was chosen, using convenient 555 circuit component values, to give One ROM plenty of time to fully boot and stabilise, and enough time for internal capacitors to meaningfully discharge during the off period, bringing VDD back down to close to zero before the next cycle.  Remember - during the period when VDD is 3.3V, all pins other than the RP2350 ADC inputs (see the note above) are fully 5v tolerant (up to 5.5V), so the testing is focused on the power-on period only.

Both Ice and Fire boards passed this test without issue.  The conclusion is that both One ROM Ice and Fire (using the STM32F4 and RP2350 respectively) can happily tolerate this over-voltage scenario under heavy use for extended periods of time.

The precise boards versions tested were:

- ice-24-f STM32F411RET6
- fire-24-d RP2350A, A4 stepping
