# CotwMod v0.5.0

## 中文

- **疊加層重寫**：改為原生 Win32 透明視窗（色鍵透明、滑鼠穿透、置頂），自動貼合遊戲畫面。修正舊版開啟疊加層後遊戲畫面被擋住的問題。只在遊戲處於前台時顯示。
- **自動讀取遊戲解析度**：直接取用遊戲視窗的實際畫面大小，視窗大小或解析度改變時即時跟隨。
- **手持武器自動識別精靈**：點「開始自動識別」，按提示切換幾次武器並按 Alt+F12。掃描、縮小範圍、指針掃描、綁定全部自動完成。如果內存中的 ID 是遊戲裝備哈希，會直接認出是哪把武器；否則每把武器只需「記住」一次。
- **即時更新**：疊加層、熱鍵、手持武器追蹤移到獨立執行緒（約 30 次/秒），換武器即時生效，主視窗最小化也照常運作。
- 新熱鍵 Alt+F12（自動識別時的「我已切換武器」）。

**已驗證（Windows 版在 Wine 下）**：疊加層透明顯示在模擬遊戲視窗上，並正確置中；解析度讀取正確；自動識別精靈 3 輪內找到手持武器並綁定指針鏈；在遊戲內換武器後約 1 秒，瞄具自動切換到新武器。**尚未在真實遊戲中運行過。**

## English

- **Overlay rewritten** as a native Win32 transparent window (colour-key transparency, click-through, topmost) fitted to the game view. Fixes the old overlay hiding the game. Shown only while the game is in front.
- **Game resolution read automatically** from the game window and followed live when the window or resolution changes.
- **Held-weapon auto-detection wizard**: click **Start detection**, then switch weapons a few times and press Alt+F12 when asked. Scanning, narrowing, pointer scan and binding are automatic. If the in-memory ID is a game equipment hash the weapon is recognised directly; otherwise each weapon is remembered once.
- **Real-time updates**: overlay, hotkeys and held-weapon tracking run on their own thread (~30 Hz), so weapon switches apply immediately, even with the main window minimised.
- New hotkey Alt+F12 ("I switched weapons" during detection).

**Verified (Windows build under Wine)**: the overlay is drawn transparently over a mock game window and correctly centred; resolution is read correctly; the wizard found the held weapon in 3 rounds and bound a pointer chain; after a weapon switch in the mock game the sight changed weapon within about 1 second. **Not yet run against the real game.**
