# Commissioning a One ROM

Commissioning a One ROM involves setting hardware properties in the RP2350's
[OTP](/docs/OTP.md) (One Time Programmable) memory.  It includes:
1. White labelling the RP2350's bootloader as One ROM
2. Setting the manufacturer and One ROM board type
3. Optionally setting the One ROM (flash) size

The data written in step 2 is signed. Commissioning a One ROM requires either:
- [an authorised signing key](#requesting-an-authorised-signing-key)
- [submitting a signing request](#submitting-a-signing-request) for your board
  to the project maintainers

## Common Use Cases

| Use Case | Action |
|----------|--------|
| Commission a single One ROM | [Submit a signing request](#submitting-a-signing-request) |
| Get an authorised signing key | [Request an authorised signing key](#requesting-an-authorised-signing-key) |
| Commission a One ROM with an authorised signing key | [Use a signing key](#using-a-signing-key) |

## Contents

- [Submitting a Signing Request](#submitting-a-signing-request)
- [Requesting an Authorised Signing Key](#requesting-an-authorised-signing-key)
- [Using a Signing Key](#using-a-signing-key)
- [CLI Options](#cli-options)
- [Correcting Commissioning Errors](#correcting-commissioning-errors)
- [Modifying One ROM Size](#modifying-one-rom-size)
- [Signing Server](#signing-server)
- [External Flash Plugin](#external-flash-plugin)

## Submitting a Signing Request

The maintainers will provide a signature on request to a person in possession
of a One ROM, allowing them to commission their own device. When commissioned
in this way, the manufacturer appears in the commissioning data as
`onerom.org`.

1. Connect the One ROM and run `onerom hardware request-signature`. For
   example:

   ```sh
   onerom hardware request-signature --board fire-40-b --size M
   ```

   This outputs a link.

2. Open the link and submit the GitHub issue that this link auto-populates.

3. The maintainers reply to the issue with the command to run to commission
   the One ROM, including the commissioning signature.  For example:

   ```sh
   onerom hardware commission --board fire-40-b --size M --manufacturer onerom.org --date 20260929 --key-id 2 --signature 3f0c...
   ```

4. Run the command. This command can **only** be used to commission the same
   One ROM. It will fail on any other One ROM. If you want to commission
   multiple One ROMs, either:
   - submit multiple signing requests
   - [request an authorised signing key](#requesting-an-authorised-signing-key)

5. Check the One ROM commissioning information validates:

   ```sh
   onerom hardware validate
   ```

## Requesting an Authorised Signing Key

The One ROM tooling only allows and validates commissioning using authorised
signing keys, to limit the potential for malicious parties to pretend a board
was manufactured by someone it wasn't.

The maintainers will consider authorising a signing key for individuals and
organisations that have One ROMs manufactured, so long as they agree to:
- exercise reasonable care in protecting their signing key from disclosure
- only sign One ROMs they have manufactured
- only sign One ROMs in their physical possession
- not sign One ROMs for other people or organisations
- inform the maintainers promptly if the key leaks so it can be revoked

A signing key is authorised for a specific set of manufacturer strings. A
signing key is typically only authorised for a single manufacturer string, but
the maintainers will consider requests for multiple strings on a case by case
basis.

Any information you submit as part of your request may be published in the One
ROM repository.

The maintainers may refuse to authorise a signing key, or revoke a signing key
at their sole discretion, for any reason, at any time.

The process for requesting a signing key be authorised is as follows. These
commands require OpenSSL 3.

1. Make an encrypted Ed25519 key file. openssl asks for a PIN, which should
   be a long random passphrase. This is your signing key and you should protect it
   from disclosure. **Never** share it.

   ```sh
   openssl genpkey -algorithm ed25519 | openssl pkcs8 -topk8 -scrypt -out acme.pem
   ```

2. Print its public key:

   ```sh
   openssl pkey -in acme.pem -pubout -outform DER | tail -c 32 | od -An -tx1 | tr -d ' \n'
   ```

   You may share the public key.

3. Choose a name for the key, for example `Acme Retro`, and print the proof:

   ```sh
   printf 'onerom-signer-v1%s' 'Acme Retro' > proof-message
   openssl pkeyutl -sign -rawin -inkey acme.pem -in proof-message | od -An -tx1 | tr -d ' \n'
   rm proof-message
   ```

4. Raise a
   [signing key request](https://github.com/piersfinlayson/one-rom/issues/new?template=signing-key-request.yml),
   listing each `--manufacturer` you want the key to be able to sign. This key will
   not be usable until the maintainers grant your request and issue a key ID.

5. Upon granting your request you will receive a unique signing key ID, which
   is required if using a [signing server](#signing-server).

## Using a Signing Key

You can commission One ROMs with an authorised signing key.

See [CLI Options](#cli-options) for guidance on the CLI options.

1. To commission a One ROM run the `onerom hardware commission` command.

   For example, to sign a fire-40-b without an external flash chip:

   ```sh
   onerom hardware commission --board fire-40-b --size M --manufacturer "Acme Retro" --key acme.pem
   ```

   It is possible to set up a signing server to handle signing, and track all
   One ROMs you commission. See [Signing Server](#signing-server).

2. Check the commissioning:

   ```sh
   onerom hardware validate
   ```

3. Optional - check OTP details:

   ```sh
   onerom inspect otp
   ```

## CLI Options

All commission commands are reached through the
[One ROM CLI](https://onerom.org/cli).

Controlling an uncommissioned One ROM without One ROM firmware installed
requires the `--unrecognised` option.

`--board` takes board type, for example `fire-40-b`. Use `onerom board list`
to show a list of all supported board types.

Fire 32 and 40 pin boards support an external 2MB flash chip being populated.
When populated alongside the primary on-MCU flash, such a One ROM board is
"Large". 32 and 40 pin board types require the `--size` option when
commissioning:
- `--size L` with the extra 2MB chip fitted
- `--size M` without it

Other boards are always size M and this does not need to be specified.

Setting size to anything other than M is **permanent** so it is recommended
to test the external flash with the
[External Flash Plugin](#external-flash-plugin) before setting the
size.  If set to `M`, the [size can be changed](#modifying-one-rom-size) later.

## Correcting Commissioning Errors

USB bootloader white-labelling details are permanent and cannot be changed once
written.

Size information can be changed for 32 and 40 pin boards if the current size is
`M`.

Board type and manufacturer details can be corrected by re-commissioning the
One ROM.  This writes a new set of commissioning data in OTP after the
existing data. The One ROM tooling uses the last commissioning instance.

To re-commission a One ROM re-run the commissioning process.

## Modifying One ROM Size

If a One ROM is already commissioned as size M (medium, no external flash
chip), its size can be modified using the `onerom hardware set-size` command.

For example, to change a fire-40-b size M into a size L run the following
command:

```sh
onerom hardware set-size --board fire-40-b --size L
```

This setting is **permanent**.  A One ROM set to a size other than M can
never be changed.

## Signing Server

To sign One ROMs using a signing server requires:
- A private server hosting the [One ROM Signing Server](/rust/signing-server/README.md).
- One or more [authorised signing keys](#requesting-an-authorised-signing-key).
- Two GitHub repos:
  - A public git repo which records hashes of all signatures produced, by key.
  - A private git repo which records all signing requests.

To sign through a [signing server](/rust/signing-server/README.md) replace
`--key acme.pem` with the server's address and the key ID. For example
`--signer https://sign.internal.example.com --key-id 256`.

## External Flash Plugin

This plugin checks that a second, external, flash chip is fitted, operational
and compatible with One ROM's size L. It is therefore useful for checking the
external flash is fully operational and compatible with One ROM before
permanently setting the One ROM's size.

For example:

```sh
onerom program --board fire-40-b --plugin usb --plugin ext-flash \
  --slot file=a_256KB_image.rom,type=27C200
onerom console
```

In the console type `y` then Enter to start the test. It ends with `PASS` or
`FAIL`.

See the [ext-flash plugin README](/plugins/user/ext-flash/README.md) for more
details.