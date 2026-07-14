# Fire 28 Rev C

A variant of the 28 pin using the RP2354B MCU, making use of its extra GPIOs to provide X pins and more efficient use of flash for most image types.  Uses a flat (inline) USB-C connector.

**Has components on both sides of the board.**

Changes from fire-28-a:
- Adds a neo-pixel LED, GPIO 44, alongside the status LED at GPIO 45.
- SEL pins have been swapped around for consistency with the latest 28, 32 and 40 pin designs.  The two right-most SEL pins are now ADC (non-5V tolerant) pins, with the othe two being 5V tolerant.
- Adds 2 more image select jumpers, matching the 4 of the 24/32/40 pins designs.  The right-most two are ADC (non-5V tolerant) pins, with the othe two being 5V tolerant.
- Adds two X pins to allow for multi-ROM slots like the 24 pin version.
- Address and CS pins have been laid out differently.
