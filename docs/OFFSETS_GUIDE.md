# 記憶體位址尋找指南（Cheat Engine）

記憶體模式讓距離、俯仰、朝向和風自動更新。本工具**只讀取**記憶體，不寫入、不注入。遊戲每次更新後位址可能失效，所以倉庫不附帶位址。

**只在單人模式下操作。** 欄位都是可選的，找到一個就能用一個。

## 需要的欄位

| 欄位 | 類型 | 說明 | 怎麼找 |
|---|---|---|---|
| `camera_pitch_deg` | float | 鏡頭俯仰，向上為正 | 抬頭/低頭，搜尋 increased/decreased value。若是弧度，加 `"scale": 57.29578` |
| `camera_yaw_deg` | float | 鏡頭朝向（羅盤角，0=北，順時針） | 原地轉身，與畫面上方羅盤對照。若方向相反用 `"scale": -1`，偏移用 `"add"` |
| `wind_from_deg` | float | 風的**來向**（羅盤角） | 風向改變時搜尋 changed value；對照 HUD 風向指示 |
| `wind_speed_ms` | float | 風速 m/s | 風力等級變化時搜尋 |
| `aim_distance_m` | float | 準星所指的距離 | 用測距儀/望遠鏡看著不同距離的物體，搜尋與顯示距離相同的 float |
| `camera_fov_deg` | float | 目前 FOV（含瞄準放大） | 開鏡/關鏡時搜尋 changed value；若找到，自動處理放大倍率 |

## 步驟

1. 開啟遊戲（無邊框視窗），進入保護區。Cheat Engine → 選擇 `theHunterCotW_F.exe`。
2. 用 First Scan / Next Scan 縮小到少數幾個位址，改變遊戲狀態確認數值同步。
3. 找到穩定的指標：右鍵位址 → *Pointer scan for this address*。重啟遊戲後用 *Rescan memory* 篩掉失效的指標鏈，留下最短的。
4. 把指標鏈寫入 `config/offsets.json`。Cheat Engine 顯示 `"theHunterCotW_F.exe"+01234567` 與 offsets `10, 28, 1A0` 時：

```json
"camera_pitch_deg": {
  "type": "float",
  "base": "0x01234567",
  "offsets": ["0x10", "0x28", "0x1A0"],
  "scale": 57.29578
}
```

語義與 Cheat Engine 相同：先讀 `[模組基址+base]`，再逐層加 offset 解引用，最後一個 offset 只相加不解引用。

## 更耐更新的寫法：AOB 特徵碼

找到存取該值的指令（右鍵 → *Find out what accesses this address*），若是 `mov rax,[rip+xxxx]` 這類 RIP 相對指令，用 AOB 定位：

```json
"wind_speed_ms": {
  "type": "float",
  "aob": "48 8B 05 ?? ?? ?? ?? 48 85 C0 74 ?? F3 0F 10 40",
  "rip_offset": 3,
  "instr_len": 7,
  "offsets": ["0x30", "0x8C"]
}
```

`rip_offset` = 4 字節相對位移在特徵碼中的位置；`instr_len` = 該指令長度。遊戲小更新後特徵碼通常仍能匹配。

## 驗證

```bat
cotwmod offsets -n 20
```

每 0.5 秒列印一次讀到的值，`None` 代表該欄位讀取失敗（遊戲未啟動、位址失效）。疊加層面板中 `●` 表示實時值、`○` 表示手動值。

把可用的 `offsets.json` 連同 `game_build`（Steam 中遊戲的建置版本號）分享到 issue，可以幫助其他玩家。
