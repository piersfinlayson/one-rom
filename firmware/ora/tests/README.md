# One ROM plugin API hardware tests

Plugins written to test the plugin API itself on a real device, rather than to
show a plugin author how to use it.  For that, see
[examples](../examples/README.md).

These are **run by hand, and are not built or run by CI**.  Each needs a One ROM,
a debug probe, and — because a test plugin displaces the USB system plugin — the
USB connector moved between programming the device and powering it to run.  None
of that fits an automated gate, and none of them belong in a release process.

Each test says, in its own README, what it was used to validate and what would
make it worth running again.  Read that first: a test here is dormant by design,
and the answer to "should I run this?" is a change to the thing it points at,
not a date.

| Test | What it covers |
| --- | --- |
| [log-test](log-test) | The plugin logging API's claim exclusion across two cores |

## Building

Both plugin types are built from this directory:

```bash
make log-test                       # log-test/build/plugin_user.bin
make log-test PLUGIN_TYPE=SYSTEM    # log-test/build/plugin_system.bin
```

The build machinery is the same `plugin.mk` the examples use, so the
prerequisites in the [examples README](../examples/README.md) apply here too.
