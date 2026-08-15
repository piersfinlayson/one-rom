# log-test

A hardware test for One ROM's plugin logging API — `ora_log_open_write`,
`ora_log_write`, `ora_log_close_write`, `ora_log_open_read`, `ora_log_read`,
`ora_log_close_read` and `ora_log_query`.  It is not an example: a plugin author
wanting to see how the API is used should read one of the
[examples](../../examples/README.md) instead.

## What this validated

The logging API on real hardware, on 2026-08-13, on a Fire 28 C: two instances
contending across cores for channel 0's write claim, the loser reading the
payload back byte for byte, unread bytes surviving a close, and a full channel
refusing a write.  Four runs — paired with boot logging and without, one
instance alone, and a firmware mutation dropping the claim check from
`ora_log_write`, which it caught.

The emulator already covers the claim tables, caller identity and every
documented return code.  What only a device can show is the SIO spinlock
arbitrating a claim between two cores, and the caller identity derived from
`SIO_CPUID`.

## When to run it again

When you change what it is pointed at: the claim tables or `ora_log_holds`, the
spinlock a claim takes, the `SIO_CPUID` caller identity, or the channel and
buffer layout — the per-core channel split being the known one.

**Not per release, and not in CI.**  It needs a device, a debug probe, and the
USB connector moved by hand between programming and running.

It does not reach the USB plugin's CDC drain, and it does not exercise `ora_log`
or `ora_err_log`, which take no claim and so cannot show anything about claim
exclusion.

## How it works

One source, flashed as **both** the system and the user plugin.  The two
instances run on different cores, so they genuinely contend: they race for the
same channel's write claim, and the SIO spinlock that arbitrates the claim, and
the core-derived caller identity behind it, only exist on real hardware.  Flashed
as one plugin only, it runs the checks a single instance can run and says so
rather than claiming a pass it did not earn.

Requires One ROM firmware v0.7.2 or later.  The `ora_log_*` calls are present
whatever `PLUGIN_LOGGING` is set to — only `ora_log` and `ora_debug_log` are
gated on it — so a plain firmware build is enough.  This plugin never calls
either, and its stack budget holds either way.

## What it checks

Neither instance is told which role it will play.  Both call
`ora_log_open_write`, and each takes its role from the result — either outcome
of the race is a valid run, and the report says which way it went.

The instance that **wins** the write claim:

- must be refused a read, holding no read claim
- waits until the other instance holds the read claim, and writes nothing until
  it does (see *Why the writer waits* below)
- writes the payload, retrying while the channel still holds boot log
- watches the channel empty, which is the other instance consuming the payload
- writes the payload again and closes the write claim, leaving those bytes
  unread
- queries the channel while holding no claim at all, and checks the invariant
  `ora_log_query` documents: `size == free + pending + 1`
- must now be refused a write

The instance that **loses** it:

- must be refused a write, both before and after taking the read claim
- takes the read claim, which must be granted while the other instance holds
  the channel for writing
- reads the payload back byte for byte, ignoring boot log ahead of it
- takes the write claim once the other instance releases it, which is how it
  observes the close
- checks that exactly the payload is still pending, and reads it back — the
  unread bytes survived the close
- fills the channel until a write is refused with `ORA_RESULT_LOG_FULL`, and
  checks `ora_log_query` agrees there is no room left
- drains the channel and releases both claims

Finally the two exchange verdicts through the channel, **each stating the role
it played**.  Two instances both reporting that they won the write claim is the
single most valuable thing this plugin can find, and neither can see it alone —
each knows only its own side of the race.

The payload starts with a marker the firmware's own text logging cannot emit,
then a byte ramp, so a channel that delivers the right bytes in the wrong order
fails rather than passes.

### Ground truth

An instrument whose only evidence runs through the mechanism it is testing will
misreport that mechanism failing.  If the write claim is granted to both cores —
the fault this plugin exists to catch — then no claim is ever refused, and a
plugin that infers "there is a second instance" purely from refusals concludes
it is running alone.  That reads to a maintainer as *you forgot to flash the
second plugin*, and sends them nowhere near the spinlock.

So the decision does not rest on the claim mechanism.
`ora_get_flash_slot_count(ORA_FLASH_SLOT_FLAG_EXCLUDE_NON_PLUGINS)` reports how
many plugin slots are installed, straight from the firmware's metadata, touching
no claim and no channel.  **Reporting SOLO requires that count to be 1.**  A
count of 2 with no claim ever refused is check 15, not a pass.

The count is of what is *installed*, not what is running, so a second plugin
that is not log-test, or one the firmware refused to launch, also reads as 2.
That is still the right answer to the only question asked of it — whether
concluding "alone" is permitted.

### Why the writer waits

Two things depend on the writing instance holding off until the other one holds
the read claim.

**A refused claim is evidence a drained channel cannot give.**  A debug probe
drains the same channel through the same read position, so a plugin that infers
a peer from an emptying channel turns an attached probe into a phantom second
instance.  `ORA_RESULT_LOG_CHANNEL_IN_USE` cannot come from a probe.

It is evidence, not proof, and the code says so where it matters:
`ora_log_claim` tests whether a channel is claimed *at all*, not whether someone
else holds it, so re-opening a claim you already hold is refused against
yourself.  Every place this plugin credits a refusal has closed, or never took,
the claim it is asking for — and the cleanup path deliberately uses the raw call
so it cannot credit a refusal it caused.

**It closes the cross-core corruption window.**  `launch_plugins_inner` starts
core 1 — and with it the system plugin — and then carries on logging about the
user plugin from core 0.  Interrupt masking does not cross cores, so a write
from the other core corrupts rather than interleaves, which would give an
intermittent check 8.  Once the other instance holds the read claim, core 0 is
executing that instance rather than the firmware, and the window has closed.

## Reading the result

**The status LED is the verdict, and needs no debug probe.**  It repeats a
count of short flashes, then pauses for about a second and a half.

Only one instance ever drives it.  `ora_set_status_led` takes no lock, and two
patterns driven at once are unreadable — worse, they can be read as a pass.  The
system instance is the reporter, and the user instance hands its verdict over
and parks as soon as that hand-over is acknowledged.  The user instance reports
for itself only once it has established that the other core is not running
log-test at all, which in the supported deployment it never is.

| Flashes | Meaning |
| --- | --- |
| 1 | PASS — two instances, every check passed |
| 2 | SOLO — one plugin slot installed, so the claim exclusion was not exercised |
| 3 | A write claim was neither granted nor refused — an unexpected result from `ora_log_open_write`, either in the opening race or when re-taking the claim later |
| 4 | A read claim was neither granted nor refused, or was still refused after the full budget |
| 5 | A write with no write claim was allowed |
| 6 | A read with no read claim was allowed |
| 7 | A write with the claim held did not store: refused outright, or still `LOG_FULL` after the full budget with the other instance draining |
| 8 | The payload did not read back: never matched, or the other instance never consumed it |
| 9 | A write claim outlived its close — `ora_log_close_write` returned an error, or the claim was still refused to the other instance afterwards |
| 10 | `ora_log_query` failed, or `size == free + pending + 1` did not hold, outside the fill check |
| 11 | Unread bytes did not survive the close: the pending count was wrong, or the bytes did not read back |
| 12 | Something was wrong at a full channel: no write was ever refused, the first write was already refused, a refusal came back as something other than `LOG_FULL`, `ora_log_query` still reported room after one, or `ora_log_query` failed or broke its invariant inside the fill check |
| 13 | No verdict was exchanged — the other instance sent nothing this build recognises, the write claim to send one never came free, or the frame was consumed by something that never answered.  The last of those is what you get when the other core is running a plugin that is not log-test, such as the USB system plugin |
| 14 | A read claim outlived its close |
| 15 | This instance never ran against a second one, but one was there: two plugin slots are installed and no claim was ever refused, or the other instance reported SOLO while this one passed.  The claim exclusion failed, or the other plugin is not log-test |
| 16 | **Both instances were granted the write claim.**  The exclusion failed open — this is the spinlock or the claim table, not the ring |
| 17 | **Neither instance was granted the write claim.**  Both were refused, which a claim table left non-zero will do.  Alone, this shows up as check 4 on one instance and check 8 on the other, and neither points at the claim |
| *steady on, no flashing* | The logging API could not be looked up — the firmware is older than v0.7.2 |

These counts are not the only thing that blinks the status LED.  The firmware's
own limp and fault patterns share the low numbers — limp mode 1 and an NMI blink
once, a hard fault twice, a bus fault three times — so a device that never got
as far as running this plugin can imitate PASS or SOLO.  The gap tells them
apart: this plugin's pause between repeats is several times longer than the
firmware's.  Where it matters, read the summary line rather than the LED.

### When both instances fail

Both codes are reported.  The LED shows the **lower** of the two: the codes run
roughly in the order the checks do, so the lower one failed earlier and the
higher one is more likely to be fallout from it.  The summary line always names
both, whichever won:

```
log-test: FAIL at check 5 - reported by the system instance, which lost the
write claim and read instead (own 15, peer 05)
```

Read the summary before acting on the LED when two codes are in play.  Lower
means earlier, which usually means closer to the cause, but it is a heuristic
and not a proof — and there is at least one case where it inverts.  If
`ora_log_close_read` fails to release, the instance holding the claim reports
14 while the instance it is now blocking reports 4, and 4 wins the LED.  The
`own 14` in the summary is what tells you which way round it was.

Checks 16 and 17 override both codes, because they name a cause neither
individual code can.  A failure reported by the other instance beats this
instance's "I never met a second one", for the same reason.

**The log channel carries that summary**, left unread for a probe attached
*after* the run.  Attach it afterwards, not during: a probe and a plugin reader
share one read position, so a probe attached while the run is in progress takes
bytes the reading instance is waiting for, and the run fails at check 8.  That
is the same constraint `ora_log_open_read` documents, and it is why the LED
rather than the log is the primary report.

### How long before the LED starts

| Run | Roughly |
| --- | --- |
| Two instances, passing | under a second |
| One plugin slot installed | 2–3 seconds — the wait for a second instance that never claims |
| Two slots, no log-test peer, reported by the system instance | 10–15 seconds — every budget has to expire before that can be concluded |
| log-test as the **user** plugin with a non-log-test system plugin | about 4 seconds — bounded by the hand-over giving up rather than by the collector's budget |

## Building

From the `tests` directory:

```bash
make log-test                       # firmware/ora/tests/log-test/build/plugin_user.bin
make log-test PLUGIN_TYPE=SYSTEM    # firmware/ora/tests/log-test/build/plugin_system.bin
```

Both are needed — the point is to run the two together.

## Flashing

Build the firmware from the repository root, and test that build rather than a
downloaded release:

```bash
make
```

Then, from the repository root:

```bash
onerom program \
  --slot file=images/test/rand_64KB.rom,type=27512 \
  --base-firmware firmware/build/onerom-rp235x.bin \
  --plugin file=firmware/ora/tests/log-test/build/plugin_system.bin \
  --plugin file=firmware/ora/tests/log-test/build/plugin_user.bin
```

**The device must have a ROM to serve.**  A config or slot set that leaves no
ROM slot — `onerom-config/blank.json`, for one — makes the firmware report
`No ROM slots to serve` and enter **limp mode 1**, which never reaches plugin
launch.  The plugins do not run at all, and limp mode 1 blinks the status LED
**once**, so the device sits there imitating a pass.  Give it a real image.

The chip type has to suit the board.  `27512` is a 28-pin part, so use a
24- or 32-pin type on those boards instead.

Run it again with boot logging on, which is the more demanding case: the channel
starts with boot log in it, which the reading instance has to drain and scan
past, and core 0 is still logging when the system plugin starts.  Both must
pass.

```bash
onerom program \
  --slot file=images/test/rand_64KB.rom,type=27512 \
  --base-firmware firmware/build/onerom-rp235x.bin \
  --plugin file=firmware/ora/tests/log-test/build/plugin_system.bin \
  --plugin file=firmware/ora/tests/log-test/build/plugin_user.bin \
  --boot-logging true
```

Boot logging is metadata, applied at programming time — there is no firmware
rebuild between these two runs.

**Flashing this as the system plugin displaces the USB plugin.**  One ROM's own
USB stack is that plugin, so while this is running the device is not on the USB
bus.  That is expected.  Stop the device and it returns to the RP2350
bootloader, where `onerom scan` finds it again — discoverability never depended
on the USB plugin.

The status LED is the discrete one.  On an RGB One ROM the NeoPixel is driven by
the `rgb` plugin, which is not present here, so it will not be lit.

### Running it alone

Flash the system plugin on its own:

```bash
onerom program \
  --slot file=images/test/rand_64KB.rom,type=27512 \
  --base-firmware firmware/build/onerom-rp235x.bin \
  --plugin file=firmware/ora/tests/log-test/build/plugin_system.bin
```

One plugin slot is installed and no claim is ever refused, so the instance
concludes it is alone, runs the same checks against itself, and reports
**2 flashes** — not a pass.  The claim exclusion needs two cores and cannot be
produced by one.

**Flash nothing else alongside it.**  The count is of plugin *slots*, and
`ORA_FLASH_SLOT_FLAG_EXCLUDE_NON_PLUGINS` includes PIO plugins as well as system
and user ones.  Adding `--plugin rgb`, or any other plugin, makes the count 2,
and the run reports **15** instead of 2 — correctly, since a second plugin
really is installed and really did not take the read claim, but it is not what
you were looking for.  This is the easiest way to be surprised on the bench.

The user plugin cannot be flashed on its own: the CLI requires a system plugin
alongside a user plugin.

## Checking the test can fail

A test that has never failed has not been shown to check anything.  Break the
claim enforcement in the firmware and reflash:

In `firmware/src/plugin.c`, in `ora_log_write`, **delete** the third of the
three guards, leaving the NULL buffer and channel ones alone:

```c
    if (!ora_log_holds(ora_log_writer, channel)) {   // delete these three
        return ORA_RESULT_INVALID_ARG;               // lines
    }                                                //
```

Rebuild the firmware and reflash.  The build shrinks by a few dozen bytes,
which is the quickest confirmation the guard really went.  A write with no
write claim is now allowed.

- **Two instances:** the instance that loses the claim race hits this on its
  very first check, before anything else can go wrong, and its code is the lower
  of the two, so the run reports **5 flashes** whichever way the race goes and
  whichever instance is the reporter.  The summary names the other instance's
  code alongside it.
- **One instance:** the same check runs after the claims are closed, so it
  reports **5 flashes** too.

Restore the guard afterwards.

## Notes on the code

- **No static RAM.**  Everything lives on the stack — the linked binary has an
  empty `.data` and `.bss`.  A user plugin gets 1KB in total, split as 512 bytes
  of static RAM and a **512 byte stack**, and putting nothing in static RAM is
  what leaves the whole stack half free.
- **The stack budget includes the firmware's own frames.**  An ORA call runs on
  the calling plugin's stack, and so does the ring code beneath it.  The deepest
  chain here is 328 bytes: 176 for the entry point, 32 and 40 for the fill and
  query helpers, then `ora_log_query` at 40 and `onerom_rtt_query` at 40.  An
  exception on top of that adds 32 bytes, or 104 with FP context — both cores
  enable the FPU — leaving roughly 80 bytes spare.  Counting only the plugin's
  own `-fstack-usage` numbers would have said 248 and been comfortably wrong.
- **`ora_log_write`, never `ora_log`.**  `ora_log`'s formatter runs on the
  calling plugin's stack, and `onerom_rtt_vprintf` alone is 144 bytes with
  `fmt_number` another 120 below it — more than the whole budget above.
  Writing byte ranges needs no scratch buffer at all, and the fixed strings here
  carry their own lengths from `sizeof`.
- **Every claim attempt goes through one wrapper**, so a refusal is recorded as
  evidence of a second instance wherever it happens rather than only where the
  caller thought to look.
- **The hand-over is acknowledged by two edges, not one.**  The channel emptying
  says only that *something* consumed the frame, and a probe or the USB plugin's
  CDC drain will do that.  A collecting instance also writes back, so the sender
  waits for the channel to refill before it treats the exchange as agreed, and
  re-sends if it does not.
  - **Known residual risk.**  The refill edge means "someone wrote to channel 0",
    not "a log-test collector answered".  `ora_log` and `ora_err_log` write that
    channel with no claim at all, so a future system plugin that logs anything
    during this window would supply a spurious refill, and the user instance
    would treat its verdict as agreed and park — silently, with no LED.  Not
    live today: the USB plugin resolves those pointers but never calls them, and
    its logging path only reads.  Anything that changes needs a real
    acknowledgement here rather than an inferred one.
- **Both instances terminate.**  Once the verdict is known the reporting
  instance blinks it and the other parks on `WFE`.  Neither touches SRAM again,
  so neither contends with the serving DMA, and neither writes to the log —
  which is what leaves the summary readable.
