# One ROM Signing Server

[OTP.md](../../docs/wip/OTP.md) describes commissioning.

## Interface

A key's URL is `https://HOST:PORT/v1/ID`, where `ID` is the key's ID.

- `GET <key URL>/public-key` returns the key's 32-byte Ed25519 public key.
- `POST <key URL>/sign` returns the 64-byte signature of a commissioning
  instance. The PIN is provided in `Authorization: Bearer PIN`. The instance's
  values are provided as JSON in the request body.

A `/sign` request body:

```json
{"chip_id": "E126C9F97C10ADAC", "board": "fire-24-f", "manufacturer": "piers.rocks", "date": "20260926"}
```

- `chip_id` is the CHIPID in the form the bootloader's USB serial number uses.
  It's 16 uppercase hex digits.
- `date` is the UTC commissioning date in the form `YYYYMMDD`.
- `dry_run` is optional and `false` when left out. A dry run returns the
  signature without adding it to the public record. It's still added to the
  private record.

The server builds the instance's message from these values. The key's ID is
the message's `COMMISSIONING_SIGNER`. Repeating a request returns the same
signature without adding another line to either record.

An error response has a one-line text body:

| Status | Cause |
| --- | --- |
| 400 | A value is refused, for example an unknown board. |
| 401 | The PIN is missing or incorrect. |
| 404 | The key or path doesn't exist. |
| 405 | The method isn't valid for the path. |
| 413 | The request exceeds 16KB. |
| 500 | The key's `key.pem` and `public.pem` don't match. |
| 500 | The server failed. |
| 503 | A record can't be written. The signature isn't returned. |

## Keys

`--keys` is a directory containing a subdirectory for each key. Key 1's
subdirectory is `keys/1`. Each subdirectory contains two files:

- `key.pem` is the encrypted private key. The server decrypts it with each
  request's PIN. It doesn't store the PIN.
- `public.pem` is the public key.

Only the user the server runs as should be able to read a key's files.

Delete a key's directory and restart the server to stop signing with it.
OTP.md's "Retiring a Key" section describes the remaining steps.

### Creating a key

Create each key on the machine that runs the server so the private key is never
written to disk unencrypted.

- Use a long random passphrase as the PIN. Anyone with a copy of `key.pem` can
  try PINs offline.
- Don't store the PIN on the server's machine.

In the directory containing `keys`, for key 1 and the signer `piers.rocks`:

1. Create the key's directory:

   ```sh
   mkdir -m 700 keys/1
   ```

2. Generate the private key. openssl prompts for the PIN twice:

   ```sh
   openssl genpkey -algorithm ed25519 | openssl pkcs8 -topk8 -scrypt -out keys/1/key.pem
   ```

3. Write the public key. openssl prompts for the PIN:

   ```sh
   openssl pkey -in keys/1/key.pem -pubout -out keys/1/public.pem
   ```

4. Print the public key in hex for the signer table,
   [`rust/app/signing-keys.json`](../app/signing-keys.json):

   ```sh
   openssl pkey -pubin -in keys/1/public.pem -outform DER | tail -c 32 | od -An -tx1 | tr -d ' \n'
   ```

5. Print the proof in hex for the signer table. The server only signs
   commissioning instances, so openssl signs the proof. openssl prompts for the
   PIN:

   ```sh
   printf 'onerom-signer-v1%s' 'piers.rocks' > proof-message
   openssl pkeyutl -sign -rawin -inkey keys/1/key.pem -in proof-message | od -An -tx1 | tr -d ' \n'
   rm proof-message
   ```

6. In the signer table, list the manufacturer strings the key may sign.
   `["*"]` allows any manufacturer, and only a key with an ID from 1 to 255
   can have it:

   ```json
   "manufacturers": ["piers.rocks"]
   ```

## The records

The server records signatures in two git repositories. Each has a file for
each key.

### The public record

`--public-record` is the server's clone of the public record repository. It
lets a retired key's genuine boards continue to validate. OTP.md's "Retiring a
Key" section describes how. Key n's file is `signatures/n.txt`. A line is the
signature's SHA-256 hash in lowercase hex:

```
9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08
```

### The private record

`--private-record` is the server's clone of the private record repository. It
records every signature the server makes, dry runs included, with the values it
covers. Key n's file is `signatures/n.jsonl`. A line is a JSON object:

```json
{"request":"live","chip_id":"FB8E5D8DD18D5EF7","board":"fire-40-a","manufacturer":"piers.rocks","date":"20260928","signature":"<128 lowercase hex digits>"}
```

- `request` is `live` or `dry-run`.
- `chip_id`, `manufacturer` and `date` are as the request provided them.
- `board` is the board's canonical name.
- `signature` is the signature in lowercase hex.

### Adding a line

For a live request the server adds the private line and then the public line.
It returns the signature once both remotes have their line. For a dry run it
adds only the private line. To add a line the server:

1. Resets the clone to the remote.
2. Appends the line.
3. Commits.
4. Pushes.

A `live` private line whose signature's hash isn't in the public file is a
signature the server never returned because it couldn't write the public
record.

Signing fails while the private remote is unreachable, dry runs included. A
live request also fails while the public remote is unreachable.

### Setting up a record

Set up each record this way. The clone must be owned by the user the server
runs as because git refuses a repository owned by another user.

1. Create the repository with an initial commit.
2. Protect its branch against force pushes and deletion. GitHub doesn't allow
   this for a private repository on GitHub Free.
3. Add a deploy key with write access to that repository only. Each repository
   needs its own deploy key because GitHub refuses a key that's already a
   deploy key on another repository.
4. Clone the repository. The server takes the clone's directory. The
   repository's URL comes from the clone's remote.
5. Set the clone's commit identity and SSH command as below. The SSH command
   uses the repository's own deploy key.

The SSH command's paths are the paths the server sees. In Docker these are paths
inside the container. The `known_hosts` file must contain the remote's host
key.

For the public record:

```sh
git clone git@github.com:OWNER/PUBLIC-REPOSITORY.git public-record
git -C public-record config user.name "One ROM signing server"
git -C public-record config user.email "EMAIL"
git -C public-record config core.sshCommand \
    "ssh -i /srv/ssh/public-deploy-key -o IdentitiesOnly=yes -o UserKnownHostsFile=/srv/ssh/known_hosts -o BatchMode=yes"
```

For the private record:

```sh
git clone git@github.com:OWNER/PRIVATE-REPOSITORY.git private-record
git -C private-record config user.name "One ROM signing server"
git -C private-record config user.email "EMAIL"
git -C private-record config core.sshCommand \
    "ssh -i /srv/ssh/private-deploy-key -o IdentitiesOnly=yes -o UserKnownHostsFile=/srv/ssh/known_hosts -o BatchMode=yes"
```

## Running

```sh
onerom-signing-server --keys keys --public-record public-record --private-record private-record \
    --tls-cert cert.pem --tls-key key.pem
```

- The server terminates TLS itself with the certificate chain in `--tls-cert`
  and its private key in `--tls-key`.
- It listens on `0.0.0.0:8443` unless `--listen` specifies another address.
- It logs to stderr at `info` level unless `RUST_LOG` specifies another level.

### Docker

The Dockerfile builds from a `git archive` export of committed files:

```sh
context=$(mktemp -d)
git archive HEAD | tar -x -C "$context"
docker build -f "$context/rust/signing-server/Dockerfile" -t onerom-signing-server "$context"
```

The image runs the server as user 10001. That user must:

- own both records
- be able to read the keys

Mount these into the container and pass the server their paths inside it:

- the keys
- both records
- the deploy keys and their `known_hosts` file
- the TLS certificate chain and private key

```sh
docker run -d --restart unless-stopped --name onerom-signing-server -p 8443:8443 \
    -v "$PWD/keys:/srv/keys:ro" \
    -v "$PWD/public-record:/srv/public-record" -v "$PWD/private-record:/srv/private-record" \
    -v "$PWD/ssh:/srv/ssh:ro" -v "$PWD/tls:/srv/tls:ro" \
    onerom-signing-server --keys /srv/keys \
    --public-record /srv/public-record --private-record /srv/private-record \
    --tls-cert /srv/tls/cert.pem --tls-key /srv/tls/key.pem
```
