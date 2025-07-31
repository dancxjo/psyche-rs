# seen

`seen` reads image bytes from a Unix socket and broadcasts a one-sentence caption.

## Input Protocol

The stream may optionally begin with a line like:

```text
@{2025-07-31T14:00:00-07:00}
```

This sets the timestamp used for incoming images. If the prefix is missing or malformed, the current local time is used.
