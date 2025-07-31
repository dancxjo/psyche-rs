# whisperd

`whisperd` streams PCM audio from a Unix socket and outputs transcriptions.
Incoming audio segments are buffered using a small queue. When the queue
fills, older segments are dropped so that recent speech is prioritized.

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
# GPU acceleration is enabled by default; use --no-gpu to disable
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

Transcriptions are timestamped using the moment the first segment arrives. The `@{timestamp}` prefix is no longer required.
