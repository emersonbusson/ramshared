# FAQ — short answers

## What is this?

When RAM is full, Linux starts using the **disk**. That feels bad.  
RamShared adds a **middle cushion**: idle **GPU** memory. If the GPU gets busy, that cushion is **given back** and data goes to disk again. Apps keep running.

## Will it break / freeze my PC?

**Designed so normal use shouldn’t freeze WSL2.**

Past freezes came from turning things off the wrong way (killing the GPU swap daemon while pages were still on that device). Today `ramshared down` always turns swap off **first**.

What you *might* notice:

- A **short slowdown** when a game on Windows reclaims VRAM (we measured up to ~1.2 s for a tiny read under hard reclaim; a full demote of hundreds of MB can take on the order of **tens of seconds**).
- That is **not** the same as “WSL dead forever.”

Heavy thrash tests: we don’t run those on the WSL you work in every day.

## Is it free RAM for games?

**No.** If a game already fills the GPU, there’s little idle memory to borrow. This is for **workstation pressure** (builds, containers, browsers) while the GPU is often idle.

## What do I need?

- Linux or **WSL2**
- NVIDIA GPU + working `nvidia-smi`
- Rust to build
- sudo

```bash
./scripts/quickstart.sh
sudo ./target/release/ramshared check
sudo ./target/release/ramshared doctor   # if check fails
```

## How do I know it worked?

```bash
swapon --show
```

Aim for three lines: **zram** (first), **GPU-backed** device (second), **disk** (last). Names vary; **order** matters.

## Is there a simple app / menu?

Yes — a small control panel (not a fancy store app):

```bash
bash scripts/safety/install-cascade-app.sh
./scripts/safety/cascade-app.sh --gui
```

Same actions as the CLI: start, stop, status, check, boot on/off.  
Under the hood it still calls the safe `ramshared up` / `down` paths.

## Turn on at WSL boot?

Yes, if you want — from the control app (**Enable boot**) or:

```bash
sudo bash scripts/safety/install-cascade-boot.sh --enable
```

Needs systemd in the distro. Config: `/etc/ramshared/cascade.conf`.  
Undo: **Disable boot** in the app, or `sudo bash scripts/safety/uninstall-cascade-boot.sh`.

## What happens when I open a game?

The daemon watches free GPU memory and latency. If the card is under pressure, it **stops using GPU as swap** (DEMOTE). Pages move to disk. Processes in WSL are **not** killed on purpose.

You may feel WSL get sluggish for a while. If it’s stuck for a long time, check `swapon --show` and logs (`journalctl -u ramshared-cascade -b` if you used boot). As a last resort on Windows: `wsl --shutdown` (only after you’ve tried `ramshared down` when you can).

## Can I install the Windows kernel driver on my physical host?

**Yes, for development and testing (MVP/Beta).** You can compile and load the driver on a physical machine, but it requires two prerequisites:
1. **Disable Secure Boot** in your motherboard UEFI/BIOS settings.
2. Enable Windows Test Mode by running:
   ```powershell
   bcdedit.exe /set "{current}" testsigning yes
   bcdedit.exe /set "{current}" nointegritychecks yes
   ```
3. Reboot your PC and compile/sign the drivers using the provided scripts.

*Note:* Force-killing the backend while the virtual disk contains active pagefile pages will cause a bluescreen (**0x7A**). Ensure you stop the pagefile usage before stopping the backend.

## Does mixing GDDR6 and DDR4 memory cause compatibility or latency issues?

No. GDDR6 (on the GPU) and DDR4 (on the motherboard) never communicate directly; each is managed by its own physical memory controller. All data transfers between them go through the PCI-Express (PCIe) bus, which standardizes the communication.

While the GPU's internal GDDR6 bandwidth is massive (e.g., 336 GB/s), the transfer speed is bounded by the PCIe bus bandwidth (e.g., PCIe Gen3 x16 is limited to ~15.8 GB/s). However, even at ~15.8 GB/s, this is **several times faster** than high-end NVMe SSD write speeds, and the access latency is in the microsecond range (µs) compared to milliseconds (ms) for SSDs and HDDs. Data integrity is fully protected by hardware-level PCIe CRC and parity checks.

## Where are the real numbers?

[validation.md](../validation.md) and [docs/reliability/](reliability/). If a number isn’t written there, treat marketing claims as suspect.
