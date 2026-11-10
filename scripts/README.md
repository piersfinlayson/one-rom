# Scripts

Helper scripts for commissioning, testing, debugging and signing.

## commission.sh

Used by piers.rocks to program, test and commission a One ROM. It displays the
values to be programmed and asks for confirmation before proceeding.

Set the following in `~/.config/onerom/commission.env`:
- `ONEROM_SIGNING_SERVER`
- `ONEROM_KEY_ID`
- `ONEROM_MANUFACTURER`
- `ONEROM_PIN`

For example:

```bash
scripts/commission.sh --board fire-24-f --size M --batch 2026111001 --rev A1
```

## run-single-test-emu.sh

Runs a single One ROM Emulator test.

For example:

```bash
scripts/run-single-test-emu.sh fire-24-a images/test/rand_8KB.rom 2364 --cs1 active_low
```

## sign-request.sh

Answers a community signing request raised on GitHub. For a valid request it:
- signs it with the piers.rocks Community key through the signing server
- posts the `onerom hardware commission` and `onerom hardware validate`
  commands to the issue
- closes the issue

It rejects and closes an invalid request.

Requires:
- curl
- jq
- onerom
- `ONEROM_SIGNING_SERVER` set to the signing server's address
- `ONEROM_GITHUB_TOKEN` set to a GitHub token that can read and write the
  repository's issues

The two variables can be set in the environment or in
`~/.config/onerom/sign-request.env`.

`--pin PIN` is the signing key's PIN. Without it `onerom hardware sign`
prompts for the PIN.

For example, to answer issue 323:

```bash
scripts/sign-request.sh 323
```
