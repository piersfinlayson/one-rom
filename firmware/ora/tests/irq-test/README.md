# irq-test

A hardware test for how One ROM delivers interrupts to plugins, and for
exclusive mode with both plugins requesting it at once.  It covers
`ora_claim_hw` for interrupts and timer alarms, `ora_set_irq_enabled`,
`ora_claim_gpio`, the plugin header's `irq_handler`, and
`ora_enter_exclusive_mode` / `ora_exit_exclusive_mode`.  It is not an example:
a plugin author wanting to see how the API is used should read one of the
[examples](../../examples/README.md) instead.

## What this validated

Interrupt delivery to plugins and exclusive mode on real hardware, on
2026-11-02, on a Fire 32 C, with firmware 0.8.1.  One paired run passed, with
200 exclusive-mode rounds and about 27 million timer interrupts on each core.
Each of the four firmware changes in [Checking the test can
fail](#checking-the-test-can-fail) failed it.  VBUS detection still entered the
bootloader afterwards, and run alone it reported SOLO.

Its first run found two firmware faults in `irq_handler_io_bank0`, which it
now checks for.  A forced GPIO interrupt locked up the core, and the handler
could disable VBUS detection.

## When to run it again

When you change what it is pointed at:

- the vector table's interrupt routing, `irq_handler_plugin`,
  `irq_to_plugin` or `irq_handler_io_bank0` in `firmware/src/plugin.c`
- how interrupt claims are recorded
- the exclusive-mode handshake in `ora_enter_exclusive_mode`, `ora_yield` or
  `yield_pause`

**Not per release, and not in CI.**  It needs a device and the USB connector
moved by hand between programming and running.

The emulator already covers the claim tables, which plugin may claim what, and
every documented return code.  What only a device can show is the vector
table, each core's NVIC, the firmware finding the interrupt from IPSR and the
plugin from `SIO_CPUID`, and the two cores' FIFO handshake.

It does not cover the USB controller interrupt, which the USB plugin uses, or a
plugin that claims an interrupt and has no handler.  Both instances share one
plugin header, so that can't be set up here.

## How it works

One source, flashed as **both** the system and the user plugin, so an instance
runs on each core.  The two keep in step using the SIO doorbells, which each
core rings on the other.  Flashed as one plugin only, it runs the checks one
instance can run and reports SOLO.

Requires One ROM firmware v0.8.1 or later.

The plugin header is written out by hand, because `ORA_SYSTEM_PLUGIN` and
`ORA_USER_PLUGIN` don't set `irq_handler`.

## What it checks

Each instance, on its own core:

1. Claims and enables `SPARE_IRQ_0` (system) or `SPARE_IRQ_1` (user) and sets
   it pending.  The handler must be called once, and `ora_set_irq_enabled` must
   enable and disable it in this core's NVIC.
2. Enables `SPARE_IRQ_2` or `SPARE_IRQ_3` in its NVIC without claiming it, and
   sets it pending.  The handler must not be called, and the firmware must have
   disabled it.
3. Claims a free GPIO and forces its edge interrupt with `INTF`, so the GPIO is
   never driven.  With `IO_IRQ_BANK0` unclaimed, the firmware must disable the
   GPIO interrupt and clear its force bit.  With it claimed, the handler must be called and the firmware
   must leave the GPIO interrupt enabled.

With both instances running:

4. The system instance claims and enables `SPARE_IRQ_4`.  The user instance
   enables it in core 0's NVIC and sets it pending there.  Neither handler may
   be called, the firmware must disable it on core 0, and it must still be
   enabled on core 1.
5. Each instance claims a timer alarm and its interrupt, and fires it every
   3µs.
6. Both instances request exclusive mode at the same moment, 200 times.  The
   delay before one instance's request changes each round, so the two requests
   meet across the whole window in which they collide.  The instance holding
   exclusive mode rings the other's doorbell and checks the other doesn't ring
   back, which it can't if it's paused.  The timer interrupt keeps firing on
   both cores, so interrupts arrive while a core is starting to pause.

Every handler call also checks it is on the instance's own core, and for an
interrupt the instance enabled.

`IO_IRQ_BANK0` stays claimed and enabled when the checks finish, for the [VBUS
step](#vbus).

## Reading the result

**The status LED is the verdict.**  It repeats a count of short flashes, then
pauses for about a second and a half.  The system instance reports for both.
The user instance sends its verdict over the doorbells, and the lower failure
of the two is shown.

| Flashes | Meaning |
| --- | --- |
| 1 | PASS.  Two instances, every check passed |
| 2 | SOLO.  One plugin slot installed, so checks 4 to 6 didn't run |
| 3 | A claim this test needs was refused, or there was no free GPIO |
| 4 | Check 1.  A claimed interrupt wasn't delivered once, or `ora_set_irq_enabled` didn't change the NVIC |
| 5 | A handler ran on the wrong core, or for an interrupt its instance never enabled |
| 6 | Check 2.  An unclaimed interrupt reached the handler or stayed enabled |
| 7 | Check 4.  The other plugin's interrupt reached a handler on this core, stayed enabled here, or was disabled on its own core |
| 8 | Check 3.  An unclaimed GPIO interrupt reached the handler, stayed enabled or stayed forced |
| 9 | Check 3.  A claimed GPIO interrupt wasn't delivered once, or the firmware changed it |
| 10 | The timer alarm interrupt didn't fire, or stopped firing during check 6 |
| 11 | `ora_enter_exclusive_mode` or `ora_exit_exclusive_mode` returned an error |
| 12 | **Both instances were in exclusive mode at once** |
| 13 | Two plugin slots are installed, but the other instance never rang a doorbell.  It stopped, or isn't irq-test |
| 14 | The user instance's verdict never arrived |
| *on, no flashing* | irq-test never started, or a plugin API lookup failed.  The firmware lights the LED at boot, and irq-test turns it off when it starts |
| *off, no flashing* | A hang.  Check 6 deadlocked, or a check never returned |

The firmware's own patterns share the low counts.  A hard fault blinks twice,
like SOLO, but with a much shorter pause.

A debug probe attached after the run can read `it_report` in each instance's
RAM, at the address `arm-none-eabi-nm` shows for the plugin's ELF.  It contains
the verdict, this instance's own and the other instance's verdicts, the
exclusive-mode rounds completed, and the timer interrupt count.

A passing run takes about two minutes before the LED starts.  The timer interrupts slow the firmware's exclusive-mode delay loops.

## Building

From the `tests` directory:

```bash
make irq-test                       # firmware/ora/tests/irq-test/build/plugin_user.bin
make irq-test PLUGIN_TYPE=SYSTEM    # firmware/ora/tests/irq-test/build/plugin_system.bin
```

## Flashing and running

Build the firmware from the repository root, and test that build rather than a
downloaded release:

```bash
make
```

Then, from the repository root:

```bash
onerom program \
  --slot file=images/test/rand_512KB.rom,type=27C040 \
  --base-firmware firmware/build/onerom-rp235x.bin \
  --plugin file=firmware/ora/tests/irq-test/build/plugin_system.bin \
  --plugin file=firmware/ora/tests/irq-test/build/plugin_user.bin
```

`27C040` is a 32-pin part, so use a 24- or 28-pin type on those boards.

This test's system plugin replaces the USB plugin and leaves VBUS detection on,
so with USB connected the device starts in the bootloader.  Unplug USB, then
power the device from its host or another 5V supply.

### VBUS

Once the LED shows PASS, plug USB in.  The device must enter the bootloader,
which `onerom scan` shows as `Stopped`.  This shows VBUS detection still works
with a plugin handling `IO_IRQ_BANK0` on core 0.

## Checking the test can fail

Each of these is a change to `firmware/src/plugin.c`.  Rebuild the firmware,
reflash, and restore the change afterwards.

| Change | Result |
| --- | --- |
| In `irq_handler_plugin`, remove the `ora_enable_irq(..., 0)` call | 6 flashes |
| In `irq_to_plugin`, accept a claim by either plugin, rather than the plugin on the interrupt's core | 8 flashes.  The unclaimed GPIO step fails first, because the other instance has already claimed `IO_IRQ_BANK0` |
| In `yield_pause`, remove the empty `asm` statement after the stub call | A hard fault |
| In `ora_enter_exclusive_mode`, make core 1 call `yield_pause()` like core 0 | A hang, with the LED off |
