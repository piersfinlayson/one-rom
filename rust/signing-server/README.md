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

The server builds the instance's message from these values. The key's ID is
the message's `COMMISSIONING_SIGNER`. Repeating a request returns the same
signature without adding another line to the record.

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
| 503 | The record can't be written. The signature isn't returned. |

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

4. Print the public key in hex for OTP.md's signer table:

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

## The record

`--record` is the server's clone of the record repository. Key n's lines are
recorded in `signatures/n.txt`.

The server records each signature before returning it so a retired key's
genuine boards continue to validate. To record a signature the server:

1. Resets the clone to the remote.
2. Appends the line.
3. Commits.
4. Pushes.

Signing fails while the remote is unreachable.

The clone must be owned by the user the server runs as because git refuses a
repository owned by another user. To set up the record:

1. Create the repository with an initial commit.
2. Protect its branch against force pushes and deletion.
3. Add a deploy key with write access to that repository only.
4. Clone the repository.
5. Set the clone's commit identity and SSH command as below.

The SSH command's paths are the paths the server sees. In Docker these are paths
inside the container. The `known_hosts` file must contain the remote's host
key.

```sh
git clone git@github.com:OWNER/REPOSITORY.git record
git -C record config user.name "One ROM signing server"
git -C record config user.email "EMAIL"
git -C record config core.sshCommand \
    "ssh -i /srv/ssh/deploy-key -o IdentitiesOnly=yes -o UserKnownHostsFile=/srv/ssh/known_hosts"
```

## Running

```sh
onerom-signing-server --keys keys --record record --tls-cert cert.pem --tls-key key.pem
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

- own the record
- be able to read the keys

Mount these into the container and pass the server their paths inside it:

- the keys
- the record
- the deploy key and its `known_hosts` file
- the TLS certificate chain and private key

```sh
docker run -d --restart unless-stopped --name onerom-signing-server -p 8443:8443 \
    -v "$PWD/keys:/srv/keys:ro" -v "$PWD/record:/srv/record" \
    -v "$PWD/ssh:/srv/ssh:ro" -v "$PWD/tls:/srv/tls:ro" \
    onerom-signing-server --keys /srv/keys --record /srv/record \
    --tls-cert /srv/tls/cert.pem --tls-key /srv/tls/key.pem
```
