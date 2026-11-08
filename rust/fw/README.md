# onerom-fw

Fetches One ROM base firmware and ROM images.

The CLI and Studio use `onerom-fw` to fetch base firmware from the release manifest and the ROM images a config refers to. [`onerom-app`](../app)'s `assemble_image` combines them into a complete image and checks it.
