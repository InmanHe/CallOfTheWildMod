# CotwMod

**[中文](#中文) | [English](#english)**

---

## 中文

《獵人：荒野的召喚》(theHunter: Call of the Wild) 單人娛樂輔助工具。Rust 編寫，單個 `CotwMod.exe`，介面支援簡體中文 / English，**不修改任何遊戲檔案**。

### 功能

| 功能 | 說明 |
|---|---|
| 🎯 **落點瞄具（任何武器）** | 不再區分武器預設。內建遊戲本身的全部 115 把武器、95 種彈藥的彈道數據（動能、彈頭質量、直徑、阻力係數、重力，以及遊戲在每個歸零檔位對每種彈藥使用的**實際發射角**）。手上拿什麼武器，就按它的歸零、風、自身下墜算出落點，透明疊加層直接畫在遊戲上。 |
| 🦌 **種群編輯器** | 可視化修改每個保護區、每個物種的公/母數量、性別轉換、稀有毛色、鑽石、巨獸；逐隻改毛色、等級、性別。自動備份、一鍵還原。 |
| 🔍 **內存掃描器** | 類 Cheat Engine 的數值掃描與指針掃描，綁定手持武器、彈藥、歸零檔位、距離、俯仰、朝向、風、FOV，讓瞄具全自動。 |
| 👁 **獵物可視距離** | 綁定地址後 400–800 m 可調並持續鎖定。 |

> 僅供單人模式使用。

### 下載與使用

到 [Releases](https://github.com/InmanHe/CallOfTheWildMod/releases) 下載 `CotwMod-*-windows-x64.zip`，解壓後執行 `CotwMod.exe`。

- 遊戲設為 **無邊框視窗**，疊加層才能顯示在遊戲上方。
- 內存功能未連接時，以 **系統管理員身分** 執行。
- 主視窗可放在遊戲後面，但不要最小化（疊加層由主視窗驅動）。
- 介面大小：右上角「界面」數值（預設 0.85）。
- 設定在 `%APPDATA%\CotwMod\settings.json`，種群備份在 `%APPDATA%\CotwMod\backups\`。

### 落點瞄具是怎麼算的

1. 從遊戲數據取出當前武器 + 彈藥 + 歸零檔位：初速 = √(2·動能/質量)，阻力 = ½ρ·Cd·截面積/質量，重力 9.82 m/s²，發射角 = 遊戲在該歸零檔位對該彈藥的設定角（含每種箭的覆寫值）。
2. 用 RK4 數值積分飛行軌跡，計入距離、俯仰（上下坡）、頂風/順風、側風。
3. 換算成螢幕像素，綠色圓圈即彈著點。

遊戲數據之外還有幾個**引擎全局常數**（發射點在視線下方的高度、阻力倍數、速度效率、風偏係數）。預設值是用遊戲全部武器的歸零表擬合出來的：弓弩 RMS 1.3 mrad，槍械中位數 0.35 mrad。這些常數對所有武器通用，**用任意一把武器在靶場校準一次，全部武器都會更準**。

**手持武器自動識別**：在「內存掃描」找到手持武器 ID 並綁定。如果這個 ID 正是遊戲裝備名稱的哈希，會直接自動識別；否則每個 ID 指定一次即可。彈藥、歸零檔位同理。

遊戲對每一發有隨機散佈、準星晃動、心率影響，這部分是遊戲刻意的隨機性，任何工具都無法預知；綠圈指向的是散佈中心。

| 熱鍵 | 功能 |
|---|---|
| Alt+F6 | 顯示/隱藏疊加層 |
| Alt+F7 / F8 / F9 / F10 | 最近用過的武器 / 彈藥 / 歸零檔位 / 瞄具倍率 |
| Alt+PgUp / PgDn, Alt+Home / End | 距離 ±1 / ±5 m |
| Alt+← / → , Alt+↑ / ↓ | 風來向 / 風力（校準模式下移動紅框） |
| Alt+F11 / Alt+Insert | 校準模式 / 記錄一發 |

### 種群編輯器

先退出遊戲 → 選保護區「讀取存檔」→ 在物種表格設定數量與珍稀度，或點物種逐隻修改 →「保存到遊戲」。第一次修改前的原檔永久保存，可一鍵還原。

### 內存掃描

地址會隨遊戲版本變化，所以程式內建掃描器讓你找一次並做指針掃描，之後長期有效。步驟見 [docs/MEMORY_GUIDE.md](docs/MEMORY_GUIDE.md)，校準見 [docs/CALIBRATION.md](docs/CALIBRATION.md)。診斷：`CotwMod.exe --selftest`。

---

## English

A single-player helper for *theHunter: Call of the Wild*. Written in Rust, one `CotwMod.exe`, Simplified Chinese / English UI, and **no game files are modified**.

### Features

| Feature | Description |
|---|---|
| 🎯 **Impact sight for any weapon** | No per-weapon presets. Ships the game's own ballistic data for all 115 weapons and 95 ammo types: kinetic energy, mass, diameter, drag coefficient, gravity, and the **actual launch angle** the game uses for every zero setting and ammo. Whatever you hold, the overlay shows where the shot will land given the zero, wind and the weapon's own drop. |
| 🦌 **Population editor** | Edit male/female counts, gender ratio, rare furs, diamonds and Great Ones per reserve and species, or edit single animals. Automatic backups, one-click restore. |
| 🔍 **Memory scanner** | Cheat-Engine-style value and pointer scanning. Bind the held weapon, ammo, zero setting, distance, pitch, yaw, wind and FOV so the sight runs fully automatically. |
| 👁 **Animal view distance** | Once its address is bound, adjust from 400 to 800 m and keep it locked. |

> For single-player use only.

### Download and use

Get `CotwMod-*-windows-x64.zip` from [Releases](https://github.com/InmanHe/CallOfTheWildMod/releases), unzip, and run `CotwMod.exe`.

- Run the game in **borderless window** mode so the overlay can draw on top of it.
- If memory features can't attach, run CotwMod **as administrator**.
- The main window may sit behind the game, but don't minimise it (it drives the overlay).
- UI size: the "UI" value at the top right (default 0.85).
- Settings: `%APPDATA%\CotwMod\settings.json`. Population backups: `%APPDATA%\CotwMod\backups\`.

### How the sight works

1. Read the current weapon + ammo + zero setting from the game data: v₀ = √(2·KE/m), drag = ½ρ·Cd·A/m, gravity 9.82 m/s², and the launch angle the game itself uses for that zero and ammo (including per-arrow overrides).
2. Integrate the trajectory with RK4, including range, incline, head/tail wind and crosswind.
3. Project to screen pixels. The green circle is the point of impact.

Beyond the game data there are a few **engine-wide constants**: launch point below the eye, drag multiplier, velocity efficiency and wind factor. The defaults were fitted to the zero tables of every weapon in the game (bows 1.3 mrad RMS, guns 0.35 mrad median). They are shared by all weapons, so **calibrating once at a range with any weapon improves every weapon**.

**Held-weapon detection**: find and bind the held-weapon ID on the Memory tab. If that ID is the game's equipment-name hash, recognition is fully automatic; otherwise assign each ID once. Ammo and zero work the same way.

The game adds random dispersion, sway and heart-rate effects to every shot by design. No tool can predict those; the green circle marks the centre of the spread.

| Hotkey | Action |
|---|---|
| Alt+F6 | Show / hide overlay |
| Alt+F7 / F8 / F9 / F10 | Recent weapon / ammo / zero setting / scope zoom |
| Alt+PgUp / PgDn, Alt+Home / End | Distance ±1 / ±5 m |
| Alt+← / →, Alt+↑ / ↓ | Wind direction / strength (move the red box in calibration mode) |
| Alt+F11 / Alt+Insert | Calibration mode / record a shot |

### Population editor

Quit the game first → choose a reserve and **Load** → set counts and rarity in the species table, or click a species to edit single animals → **Save to game**. The first original file is kept forever and can be restored in one click.

### Memory scanning

Addresses change with game updates, so CotwMod has a built-in scanner: find each value once, run a pointer scan, and the binding keeps working. See [docs/MEMORY_GUIDE.md](docs/MEMORY_GUIDE.md) and [docs/CALIBRATION.md](docs/CALIBRATION.md). Diagnostics: `CotwMod.exe --selftest`.

---

## Build / 編譯

```bash
cargo test                 # core logic (Linux or Windows)
cargo build --release      # on Windows
./build-windows.sh         # cross-compile from Linux (mingw-w64 + rust-src)
python3 tools/gen_ballistics.py <extracted-game-files>   # regenerate data/ballistics.json
```

`tools/` also contains a differential test that checks the Rust population editor against the original Python APC field by field.

## Credits / 致謝

MIT. Population format and species data: [APC Revived](https://github.com/RyMaxim/apc) (Ryan Maxim, MIT) and [deca](https://github.com/kk49/deca) (Krzysztof Kamieniecki, MIT). Weapon/ammo ballistic numbers come from the game's own data files (build 2777186, via the extracted files shipped with cpypasta's COTW Mod Builder). Fur seed algorithm by xpltive / 0xSthSth1337. See [THIRD_PARTY.md](THIRD_PARTY.md).
