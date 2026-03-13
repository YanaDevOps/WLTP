# WLTP Project Setup Guide

## Project Status

### ✅ Completed Components

All core implementation has been completed:

#### Rust Backend (src-tauri/)
- **types.rs**: Core data structures (TraceSession, HopSample, HopInterpretation, SessionSummary)
- **traceroute.rs**: ICMP traceroute implementation for Windows/macOS with continuous measurement
- **interpretation.rs**: Rule-based diagnostic engine with human-readable explanations
- **commands.rs**: Tauri command handlers for frontend communication
- **main.rs**: Application entry point with tokio runtime
- **lib.rs**: Library exports

#### Frontend (src/)
- **main.tsx**: React entry point
- **App.tsx**: Main application component with:
  - Diagnostic view (target input, trace controls, real-time updates)
  - Settings view (theme, explanation level, measurement parameters)
  - Summary card with color-coded status
  - Hops table with tooltips and metrics
- **lib/tauri.ts**: Tauri API wrapper with type-safe commands
- **types/global.d.ts**: TypeScript type definitions
- **index.css**: Tailwind CSS imports

#### Configuration Files
- **package.json**: npm dependencies (React 18, Tauri 2.x, TypeScript, Tailwind)
- **tsconfig.json**: TypeScript configuration
- **vite.config.ts**: Vite bundler configuration
- **tailwind.config.js**: Tailwind CSS configuration
- **tauri.conf.json**: Tauri app configuration
- **Cargo.toml**: Rust dependencies

### 🔨 System Requirements for Build

#### To Build the Rust Backend

**Windows** (required for this setup):
- Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/downloads/) with "C++ build tools"
  - During installation, select "Desktop development with C++"
  - OR install [Build Tools for Visual Studio](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
- After installation, restart your terminal/command prompt

**Alternative** (GNU toolchain):
```bash
rustup toolchain install stable-x86_64-pc-windows-gnu
rustup default stable-x86_64-pc-windows-gnu
```

**macOS**:
- Xcode Command Line Tools: `xcode-select --install`
- Homebrew dependencies (if needed)

#### To Run the Application

**Permission Requirements**:
- Windows: **Run as Administrator** (required for raw ICMP sockets)
- macOS: `sudo` privileges (required for raw sockets)

### 📋 Build & Run Commands

Once system requirements are met:

```bash
# Install npm dependencies (already done)
npm install

# Build frontend
npm run build

# Run Tauri development server (will build Rust backend)
npm run tauri dev

# Or build for production
npm run tauri build
```

### 🎨 Features Implemented

1. **Network Diagnostics**:
   - ICMP traceroute with TTL-based hop discovery
   - Continuous measurement with configurable intervals
   - Automatic statistics calculation (loss, latency, jitter)
   - Real-time updates via Tauri events

2. **Smart Interpretation Engine**:
   - Distinguishes between ICMP rate limiting and real packet loss
   - Identifies latency increase points in the route
   - Detects high jitter and explains its impact
   - Confidence-weighted diagnoses

3. **User-Friendly Interface**:
   - Clean, modern UI with Tailwind CSS
   - System/Light/Dark theme support
   - Color-coded status indicators
   - Tooltips explaining each metric
   - Real-time progress updates

4. **Export Functionality**:
   - Self-contained HTML reports for support tickets
   - JSON export for technical integration
   - Include interpretations, metrics, and raw data

### 📁 Project Structure

```
WLTP/
├── src-tauri/              # Rust backend
│   ├── src/
│   │   ├── main.rs        # Entry point
│   │   ├── lib.rs         # Module exports
│   │   ├── types.rs       # Core types
│   │   ├── traceroute.rs  # ICMP implementation
│   │   ├── interpretation.rs  # Analysis engine
│   │   └── commands.rs    # Tauri commands
│   ├── Cargo.toml         # Rust dependencies
│   └── tauri.conf.json    # App config
├── src/                   # React frontend
│   ├── main.tsx          # Entry point
│   ├── App.tsx           # Main component
│   ├── lib/
│   │   └── tauri.ts      # API wrapper
│   └── types/
│       └── global.d.ts   # Type definitions
├── package.json          # npm config
├── tsconfig.json         # TypeScript config
├── vite.config.ts        # Vite config
└── tailwind.config.js    # Tailwind config
```

### 🐛 Known Issues & Workarounds

1. **Raw Socket Permissions**:
   - Windows: Must run as Administrator
   - macOS: Must run with sudo
   - Error: `PermissionDenied` if not elevated

2. **IPv6 Support**:
   - Currently IPv4 only
   - IPv6 returns "not yet supported" error

3. **Windows Firewall**:
   - May block ICMP packets
   - Allow the application through Windows Defender Firewall

### 🧪 Testing Checklist

After successful build:

- [ ] Trace to `google.com` works
- [ ] Trace to `8.8.8.8` works
- [ ] Intermediate hop timeouts show "may be normal" message
- [ ] High loss at destination shows Critical status
- [ ] Export HTML generates valid report
- [ ] Export JSON generates valid data
- [ ] Theme switching works (System/Light/Dark)
- [ ] Real-time updates appear in table
- [ ] Settings persist across restarts

### 📝 Next Steps

1. **Install Visual Studio Build Tools** (Windows)
2. **Run `npm run tauri dev`** to start development server
3. **Test with known good targets** (google.com, cloudflare.com)
4. **Test with problem targets** to verify interpretation engine
5. **Build production release** with `npm run tauri build`

### 🆘 Support

For issues:
1. Check Windows Event Viewer for ICMP/DNS errors
2. Run Windows Network Diagnostics
3. Test with `ping` and `tracert` commands for comparison
4. Check firewall logs for blocked packets

---

**Project**: WLTP - Modern WinMTR for Windows/macOS  
**Status**: ✅ Implementation Complete, 🔄 Build Setup Pending  
**Last Updated**: 2025-03-13
