# CotwMod v0.6.0

## 中文

- **新的「快速開始」首頁**：一張檢查清單加一個按鈕。遊戲是否連上、彈道數據是否讀到、瞄具是否開啟、哪些數值已自動讀取，一眼看完。
- **一鍵設定引導**：不再需要自己用內存掃描找數值。點「一鍵設置」，照提示在遊戲裡做動作並按 Alt+F12：
  - 手持武器：換幾次武器
  - 歸零檔位：調到最低檔，再逐檔調高
  - 鏡頭俯仰：抬頭到頂、低頭到底（自動判斷弧度/角度和正負方向）
  - 鏡頭視角：開鏡、收鏡（自動選出與你設定的 FOV 相符的那個）
  - 測距距離：用測距儀量 2–4 個目標，輸入顯示的數字
  掃描、篩選、驗證、指針掃描、綁定、存檔全部自動，結果在遊戲重啟後仍有效。
- **提示直接顯示在遊戲畫面上**：引導時不用切回 CotwMod 視窗（測距那一步除外）。
- 疊加層預設開啟；熱鍵清單改為中英文說明。

**仍需手動的部分**：風向和風力（遊戲 HUD 沒有數字可供自動驗證，用 Alt+方向鍵設定）；動物可視距離（在「內存掃描」頁，方法見 docs/MEMORY_GUIDE.md）。XeSS/DLSS 無法對這款遊戲強制開啟，本工具不處理畫面升頻。

**已驗證**：引導的五個項目在模擬進程上全部綁定到正確位址（自動化測試，含干擾數值、反向俯仰、水平 FOV 等誘餌）；Windows 版在 Wine 下完成手持武器引導並在模擬遊戲視窗上顯示提示。**尚未在真實遊戲中運行過**——某一項失敗時重新設置該項即可，其餘手動方式仍然保留。

## English

- **New Quick start home page**: one checklist and one button. Shows at a glance whether the game is attached, ballistic data is loaded, the sight is on, and which values are read automatically.
- **One-button guided setup**: no more manual memory scanning. Click **Set up everything that is missing**, then do what the prompt says in game and press Alt+F12:
  - Held weapon: switch weapons a few times
  - Zero level: lowest level, then one level up at a time
  - Camera pitch: look fully up, fully down (radians/degrees and sign are detected)
  - Camera FOV: scope in, scope out (the candidate matching your configured FOV is chosen)
  - Range distance: range 2–4 targets and type the number shown
  Scanning, filtering, verification, pointer scan, binding and saving are automatic and survive game restarts.
- **Prompts are drawn on the game screen**, so you stay in the game during setup (except for typing distances).
- Overlay is on by default; the hotkey list now has proper descriptions.

**Still manual**: wind direction and strength (the HUD shows no number to verify against; use Alt+arrow keys) and animal view distance (Memory tab, see docs/MEMORY_GUIDE.md). XeSS/DLSS cannot be forced on in this game; this tool does not do upscaling.

**Verified**: all five guided tasks bind the correct address on a simulated process (automated test with noise values and decoys such as inverted pitch and horizontal FOV); the Windows build completed the held-weapon guide under Wine and showed the prompt over a mock game window. **Not yet run against the real game** — if a step fails, redo that step; the manual methods remain available.
