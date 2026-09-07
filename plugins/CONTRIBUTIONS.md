# Plugin Contributions

One ROM hosts plugins written by third-parties at
[images.onerom.org](https://images.onerom.org/plugins/plugins.json). This
means they appear in One ROM Web's plugin drop-downs, and can be referred
to using a short name with the CLI's `--plugin` option:

```bash
onerom program --plugin usb --plugin third-party-plugin ...
```

The plugin source stays in the third-party's own repository.

## Contribution Steps

1. Write and test a plugin.
2. Include a [`plugin-meta.json`](#plugin-metajson) file in your plugin directory.
3. Commit and push your plugin source to a public repository.
4. Tag the commit you want published.
5. [Raise an issue](#raising-an-issue) requesting your plugin be published.

## plugin-meta.json

A plugin contribution must include a `plugin-meta.json` file in the plugin's
root directory.  Here is an example:

```json
{
    "name": "trs80-amaze",
    "display_name": "TRS-80 Amaze",
    "description": "Provides an amazing TRS-80 experience with One ROM",
    "author": "Ada Lovelace",
    "source": "https://github.com/ada/trs80-amaze-plugin",
    "license": "MIT",
    "build_output": "build/plugin_user.bin"
}
```

| Field | Meaning |
| --- | --- |
| `name` | The short name proposed for this plugin. Lower case, dashes not spaces. Scoped to the machine or purpose |
| `display_name` | Short display string, to be displayed in the web programmer drop-downs. May include spaces |
| `description` | One line description of the plugin's purpose |
| `author` | The plugin author's name or a handle |
| `source` | The repository the plugin is built from |
| `license` | An [SPDX identifier](https://spdx.org/licenses/), for example `MIT`. Must permit One ROM to build, distribute and host the plugin |
| `build_output` | Path to the binary `make` produces, relative to the plugin directory. |

All of the above information is included in the plugin's manifest hosted
publicly at https://images.onerom.org.

Notes:

- There is no plugin version field.  The plugin's version, its type, and the
  minimum One ROM firmware version it needs are all read from the built
  plugin binary.

- `source` and `license` are fixed for the life of a plugin. If you want to
  change either, [raise an issue](#raising-an-issue)

## Versioning

To release a new version of your plugin, follow the
[raising an issue](#raising-an-issue) process again.

## Raising an Issue

Raise an issue [here](https://github.com/piersfinlayson/one-rom/issues/new/choose).

Include:
- The URL of a public repository holding your plugin's source.
- The directory inside the repository containing the plugin.
- The tag to publish.
- The name you would like the plugin published under.

The plugin must include a:
- Makefile
- valid plugin header
- [`plugin-meta.json`](#plugin-metajson) file.

The plugin must compile against the latest released version of the One ROM source
tree.

## Publishing

The project maintainer will respond to your issue with any questions, and,
assuming your plugin submission is accepted, build and publish your plugin.

## Yanking and Modifying Plugins

Please [raise an issue](#raising-an-issue) with your request.

## Plugin Lifetime

The lifetime of a published plugin is tied to the availability of the plugin's
source repository, and the availability of the specific commit hash it was built
from. If either become unavailable the plugin may be yanked from
https://images.onerom.org.

## Disclaimer

One ROM's maintainer reserves the right to refuse a plugin submission at their
discretion, and to yank any plugin from https://images.onerom.org at any time,
for any reason.

One ROM's maintainer provides no warranty for third-party plugins. The plugin's
license governs any warranties or lack thereof.

In particular, the maintainer performs no review or auditing of third-party
plugins that are published at https://images.onerom.org. 