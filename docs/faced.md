# faced

`faced` is a simple Unix socket daemon that performs face recognition on JPEG images using OpenCV and stores face vectors in Qdrant via `rememberd`.

Send a complete JPEG to the socket and it responds with a single line listing recognized faces. When no faces are present the line `(no faces detected)` is sent.

```bash
# send an image and print result
cat image.jpg | socat - UNIX-CONNECT:/run/psyche/faced.sock
```

Unknown faces are automatically stored via `rememberd` and labeled with a `Stranger` prefix. You can later associate a name with the generated ID by writing a `face_alias` entry through `rememberd`.
