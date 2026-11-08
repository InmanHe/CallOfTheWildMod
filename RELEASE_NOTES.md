# CotwMod v0.3.0

## 中文

**落點瞄具不再區分武器。** 內建遊戲本身的 115 把武器、95 種彈藥的彈道數據，包括動能、彈頭質量、直徑、阻力係數、重力，以及遊戲在每個歸零檔位對每種彈藥實際使用的發射角。不論手上拿弓、弩、步槍、手槍還是霰彈槍，都按當前歸零、風和該武器自身的下墜計算落點。

- 武器清單可搜尋，彈藥只列出該武器可用的，歸零檔位來自武器本身的數據。
- 手持武器 / 彈藥 / 歸零檔位可從內存自動同步。如果內存 ID 是遊戲裝備哈希會自動識別，否則每個 ID 指定一次即可。
- 引擎全局常數（發射點高度、阻力倍數、速度效率、風偏係數）預設由遊戲全部歸零表擬合：弓弩 1.3 mrad RMS，槍械中位數 0.35 mrad。用任意武器校準一次，所有武器同時變準。
- 介面預設縮小（0.85，右上角可調），視窗預設 1100×720。
- 雙語 README。

種群編輯器、內存掃描器、指針掃描、可視距離 400–800 m 鎖定等功能同 v0.2.0。

**使用**：解壓後執行 `CotwMod.exe`。遊戲設為無邊框視窗；內存功能需要時以系統管理員身分執行；修改種群前先退出遊戲。

**已驗證**：種群讀寫與原版 APC 輸出逐欄位一致；Windows 內存讀取、掃描、指針掃描、疊加層已在 Wine 下對模擬遊戲進程驗證；彈道模型可重現遊戲自身的歸零表。**尚未在真實遊戲中測試**，首次使用前請備份存檔資料夾。

## English

**The impact sight no longer depends on weapon presets.** It ships the game's own ballistic data for all 115 weapons and 95 ammo types: kinetic energy, mass, diameter, drag coefficient, gravity, and the launch angle the game actually uses for every zero setting and ammo. Bow, crossbow, rifle, handgun or shotgun: the point of impact follows the current zero, the wind and that weapon's own drop.

- Searchable weapon list; only compatible ammo is listed; zero settings come from the weapon's own data.
- Held weapon / ammo / zero setting can sync from memory. IDs that are game equipment hashes are recognised automatically; other IDs are assigned once.
- Engine-wide constants (launch height, drag multiplier, velocity efficiency, wind factor) default to a fit over every zero table in the game: bows 1.3 mrad RMS, guns 0.35 mrad median. Calibrating once with any weapon improves all of them.
- Smaller default UI (0.85, adjustable at the top right); default window 1100×720.
- Bilingual README.

Population editor, memory scanner, pointer scan and the 400–800 m view-distance lock are unchanged from v0.2.0.

**Use**: unzip and run `CotwMod.exe`. Run the game borderless; run CotwMod as administrator if memory features can't attach; quit the game before editing populations.

**Verified**: population edits match the original APC output field by field; Windows memory reading, scanning, pointer scanning and the overlay were tested under Wine against a mock game process; the ballistic model reproduces the game's own zero tables. **Not yet tested in the real game** — back up your save folder before first use.
