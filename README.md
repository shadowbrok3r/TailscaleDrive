
# Taildrop Watcher (Rust)

A lightweight, ergonomic daemon written in Rust / Egui that listens for **Taildrop'd files**. 

It bypasses the need to manually run `tailscale file get` by hooking directly into the Tailscale local API event bus to detect incoming files in real-time.

## Why?
- Receiving files on Linux via TailDrop is not difficult by any stretch, but when I'm trying to send files back and forth from device to device quickly, it gets tedious. 
- When I have an IOS device and cannot use airdrop, (I dont even own a Mac, but airdrop IS a nice feature for IOS->IOS) then have to decide whether to use Google Drive, external NVME, or some other attrocious way of bridging the gap from IOS to the rest of the world
- You have to know a file is coming and run a CLI command to fetch it. 
- This project aims to make the experience a little more seamless, by watching for events and handling files with less friction.

## How it works
- **Event Stream:** Uses `tokio` and `hyper` to stream events from the Tailscale daemon
- **Direct Socket Connection:** Connects via Unix Domain Socket (`/var/run/tailscale/tailscaled.sock`).
- **Tailscale Socket Schema:** Parses Tailscale JSON events, distinguishing between `IncomingFiles` (auto-saved) and `FilesWaiting`.

## Running
- **Tailscale** is installed and running (`systemctl status tailscaled`).
- TODO

*You can add your user to the tailscale group to run without sudo:*
```bash
sudo usermod -aG tailscale $USER
```


3. **Send a file:**
Open Tailscale on your phone or another device, select a file, and send it to this machine. You will see a notification in the terminal immediately.

## Technical Info

This application functions by implementing a custom HTTP Connector over a Unix Socket. It hits the following internal Tailscale endpoint:

* **Endpoint:** `GET /localapi/v0/watch-ipn-bus`
* **Protocol:** Streaming JSON (Newline Delimited)

It listens specifically for the `IncomingFiles` array and the `FilesWaiting` map in the JSON payload.

## ROADMAP

* [x] Connect to Unix Socket
* [x] Parse `watch-ipn-bus` event stream
* [ ] **Windows Support** (Named Pipe connector)
* [ ] **Notifications:** Integrate with system-level notifications (DBus/Notify-send).
* [ ] Will I be able to implement the same experience on the receiving end of an IOS device? No idea yet.. 
* [ ] Figure out how to get egui running in the system tray area..?
