# whisperd

`whisperd` streams PCM audio from a Unix socket and outputs transcriptions.

## Systemd

To run `whisperd` as a service install the unit file below and enable it:

```ini
# /etc/systemd/system/whisperd.service
[Unit]
Description=Whisper Audio Transcription Daemon
After=network.target

[Service]
Type=simple
User=whisper
ExecStart=/usr/local/bin/whisperd \
  --whisper-model /opt/whisper/model.bin \
  --socket /run/psyched/ear.sock \
  --daemon
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

Reload and start the service:

```bash
sudo systemctl daemon-reexec
sudo systemctl enable --now whisperd
```

You can also generate the unit file with:

```bash
whisperd gen-systemd > whisperd.service
```

## Input Protocol

The stream may optionally begin with a line like:

```text
@{2025-07-31T14:00:00-07:00}
```

This sets the timestamp used for incoming audio. If the prefix is missing or malformed, the current local time is used.
