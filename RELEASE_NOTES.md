# CotwMod v0.4.0

## 中文

**彈道數據改為直接讀取你安裝的遊戲，並自動校準。**

- **自動讀取本機遊戲檔案（唯讀）**：啟動時自動找到遊戲資料夾（正在運行的遊戲、Steam 各遊戲庫、Epic，或手動填寫），從 `archives_win64` 讀出當前遊戲版本**所有**武器與彈藥的彈道數據，包括新 DLC 槍械。結果會快取，遊戲更新後自動重新讀取。內建數據只在讀取失敗時作為備用。
- **動態自動校準**：每次啟動用當前遊戲版本全部武器的歸零表自動擬合引擎常數，不需要開槍，並自動剔除不一致的條目（目前：弓弩約 1.2 mrad RMS，槍械中位數約 0.5 mrad）。在靶場每記錄一發射擊，背景自動重新擬合。
- 修正：名稱含連字號的武器調校檔（如 45-70 槓桿步槍）之前會被漏掉，現在共 116 把武器。

**已驗證**：把提取出的遊戲檔案重新打包成真正的 TAB/ARC/AAF/SARC 容器，先用 deca 自身的讀取器逐檔核對，再讓 Windows 版程式在 Wine 下從中讀取：116 把武器、95 種彈藥、每個數值完全一致。程式也能透過正在運行的遊戲進程自動找到遊戲資料夾。**尚未在真實遊戲上運行過**；若讀取失敗，介面會顯示原因並改用內建數據。

**使用**：解壓後執行 `CotwMod.exe`。遊戲設為無邊框視窗；內存功能需要時以系統管理員身分執行；修改種群前先退出遊戲。

## English

**Ballistic data is now read from your installed game and calibrated automatically.**

- **Reads your own game files (read-only)**: at start-up the game folder is found automatically (running game, every Steam library, Epic, or entered manually), and the ballistics of **every** weapon and ammo type in your game version are read from `archives_win64`, new DLC guns included. Results are cached and re-read after game updates. Built-in data is only a fallback.
- **Dynamic auto-calibration**: on every start the engine constants are fitted to the zero tables of all weapons in your game version, with no shooting needed and inconsistent entries rejected automatically (currently about 1.2 mrad RMS for bows, 0.5 mrad median for guns). Every shot recorded at a range triggers a background refit.
- Fix: weapon tuning files with a hyphen in their name (e.g. the 45-70 lever-action) were missed before; now 116 weapons.

**Verified**: extracted game files were re-packed into genuine TAB/ARC/AAF/SARC containers, checked file by file with deca's own readers, and then read by the Windows build under Wine: 116 weapons, 95 ammo types, every value identical. The game folder is also found from the running game process. **Not yet run against the real game**; if reading fails, the UI shows why and falls back to the built-in data.

**Use**: unzip and run `CotwMod.exe`. Run the game borderless; run as administrator if memory features can't attach; quit the game before editing populations.
