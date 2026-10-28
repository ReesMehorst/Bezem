# Bezem 🧹

[![License: MIT](https://shields.io)](https://opensource.org)
[![Platform: Windows](https://shields.io)]()

**Bezem** (Dutch for *Broom*) is a lightweight, blazing-fast, and open-source system optimization and privacy tool designed to sweep away digital clutter from your Windows environment. 

Many PC cleaners are bloated with telemetry, advertisements, and unnecessary background processes. Bezem does exactly what a cleaner should do: target and remove redundant temporary data safely, transparently, and with zero overhead.

---

## 📸 Interface

# screenshot or a GIF of Bezem 

---

## Key Features

* **Smart Temp Purge:** Scans and safely clears out user and system-level `%TEMP%` directories.
* **Privacy Shredder:** Wipes browser caches, tracking cookies, and local session logs.
* **System Refresher:** Flushes the Windows DNS cache and clears out obsolete prefetch data.
* **Safe-by-Design Execution:** Intelligently skips files currently in use by active system processes to prevent OS instability or crashes.
* **Zero Bloat:** No background telemetry, no notifications, and absolute transparency.

## How It Works

Bezem sequentially scans predefined Windows storage pathways that accumulate junk data over time. It calculates potential storage recovery, executes a safe unlink/deletion process, and provides a clear breakdown of successful removals versus skipped system-locked files.

---

## 🛠️ Installation & Setup

### Prerequisites
* **Operating System:** Windows 10 / 11
* **Permissions:** Administrator privileges are required to clean system-level directories (like `C:\Windows\Temp`).

### Option 1: Running from Source
If you want to run or develop Bezem locally, clone the repository and install the dependencies:

```bash
# Clone the repository
git clone https://github.com

# Navigate into the project directory
cd Bezem

# Install dependencies (uncomment if using a requirements.txt file)
# pip install -r requirements.txt

# Run the application (Right-click your terminal and Run as Administrator)
python main.py
```
*(Note: Change `python main.py` to match your core language execution command, such as `dotnet run` for C#).*

### Option 2: Using the Executable
1. Download the latest compiled release from the **Releases** tab.
2. Right-click `Bezem.exe` and select **Run as Administrator**.
3. Follow the on-screen prompts or GUI buttons to scan and sweep your system clean.

---

## Contributing

Contributions are what make the open-source community such an amazing place to learn, inspire, and create. Any contributions you make to improve Bezem's sweeping algorithms, performance, or UI are highly appreciated.

1. Fork the Project
2. Create your Feature Branch (`git checkout -b feature/AmazingFeature`)
3. Commit your Changes (`git commit -m 'Add some AmazingFeature'`)
4. Push to the Branch (`git push origin feature/AmazingFeature`)
5. Open a Pull Request

## License

Distributed under the MIT License. See `LICENSE` for more information.

---
*Disclaimer: Bezem is an open-source tool. While it is designed with safety guidelines in mind to only remove temporary junk files, always ensure important data is backed up before running system optimization tools.*
