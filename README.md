# CallOfTheWildMod

《獵人：荒野的召喚》(theHunter: Call of the Wild) 外部輔助工具，**不修改任何遊戲檔案**：

1. **弓弩預測落點瞄具**：複合弓、傳統弓（反曲弓/長弓）、十字弩。透明疊加層實時畫出箭/弩箭的落點，計入
   - 遊戲自身的**歸零距離**（每把武器的歸零選項，與 Zeroing 技能一致）
   - 距離、俯仰角（上坡/下坡）、**風向與風速**（頂風/順風改變下墜，側風產生偏移）
   - 每種武器 + 箭矢組合可在靶場**校準**，擬合出遊戲實際使用的初速、阻力和風偏係數
2. **種群數量與珍稀度控制**：用一個 YAML 配置檔設定每個保護區、每個物種的公/母數量、性別比例、稀有毛色、鑽石、巨獸(Great One) 數量，一條命令套用，自動備份、可一鍵還原。

> ⚠️ 只建議在單人模式使用。多人遊戲中使用種群修改或瞄準輔助對其他玩家不公平。

---

## 運作方式（為什麼不需要改遊戲檔案）

| 功能 | 實作 | 接觸到的東西 |
|---|---|---|
| 瞄具 | 獨立的置頂、滑鼠穿透透明視窗，畫在遊戲畫面上方 | 只**讀取**遊戲進程記憶體（可選），不寫入、不注入 |
| 種群 | 修改存檔資料夾內的 `animal_population_N` 存檔 | 只動你的**存檔**，遊戲安裝目錄完全不碰 |

種群存檔（ADF 格式）的解析使用 [APC – Animal Population Changer Revived](https://github.com/RyMaxim/apc)（MIT），作為 git 子模組放在 `third_party/apc`。

---

## 安裝（Windows）

需要 Python 3.12 以上。

```bat
git clone --recurse-submodules https://github.com/InmanHe/CallOfTheWildMod
cd CallOfTheWildMod
pip install -e .
```

遊戲設定為 **無邊框視窗 (Borderless)** 或視窗模式 — 疊加層無法畫在獨佔全螢幕上。

---

## 一、預測落點瞄具

```bat
cotwmod aim
```

畫面中央小十字 = 武器自身的瞄準基準（歸零針/中心點）；**綠色圓圈 = 箭的實際落點**。把綠圈放在獵物要害上放箭即可。左上角面板顯示武器、歸零、距離、風、需要的抬高量；`●` 表示該數值是從遊戲實時讀取，`○` 表示手動輸入。

### 熱鍵（預設需按住左 Alt）

| 熱鍵 | 功能 |
|---|---|
| Alt+F6 | 顯示/隱藏疊加層 |
| Alt+F7 / F8 / F9 | 切換武器 / 箭矢 / 歸零距離（**務必與遊戲內設定一致**） |
| Alt+F10 | 切換倍率（十字弩瞄準鏡） |
| Alt+PgUp / PgDn | 距離 ±1 m |
| Alt+Home / End | 距離 ±5 m |
| Alt+← / → | 風向（相對於你的朝向）每次轉 45° |
| Alt+↑ / ↓ | 風力等級 |
| Alt+F11 | 校準模式開關 |
| Alt+Insert | 記錄一次校準射擊 |

熱鍵可在 `config/settings.json` 修改；`hotkey_modifier` 設為 `null` 可取消 Alt。

### 輸入來源：手動 或 實時記憶體讀取

- **手動模式**（開箱即用）：用遊戲的測距儀/望遠鏡讀距離，再用熱鍵輸入距離與風。
- **記憶體模式**：在 `config/offsets.json` 填入距離、鏡頭俯仰/朝向、風向、風速的位址後，這些數值自動實時更新，風向會自動換算成相對方向。每個欄位獨立：找到幾個就用幾個，其餘仍用手動值。
  遊戲更新會改變位址，所以倉庫**不附帶**位址；按 [docs/OFFSETS_GUIDE.md](docs/OFFSETS_GUIDE.md) 用 Cheat Engine 找，建議用 AOB 特徵碼，遊戲小更新後通常仍有效。
  `cotwmod offsets` 可實時列印讀到的數值來驗證。

### 校準（達到精準的關鍵）

`config/weapons.json` 中的初速/阻力只是**起始估計值**，遊戲內部數據並未公開。精準度來自校準：在靶場對已知距離射擊，記錄箭落點，擬合出遊戲實際的彈道參數。詳見 [docs/CALIBRATION.md](docs/CALIBRATION.md)。

```bat
cotwmod calibrate                     # 擬合並寫入 config/calibrated.json
cotwmod table -w hawkedge_cb70 -a arrow_420gr -z 40 --wind 4   # 檢查彈道表
```

### 關於「100% 精準」

只要參數校準正確，預測的是**箭的平均落點**，數學上與遊戲模型一致（RK4 數值積分，誤差遠小於 0.01 mrad）。但遊戲本身對弓箭加入了**隨機散佈**、準星晃動、心率影響，這些是遊戲故意設定的隨機性，任何外部工具都無法預知。實際效果：綠圈精確指向散佈中心，散佈本身由你的技能/姿勢/呼吸決定。

---

## 二、種群數量與珍稀度控制

**先關閉遊戲**（遊戲退出時會重寫種群存檔）。

```bat
cotwmod pop reserves                  # 列出保護區代碼
cotwmod pop species hirsch            # 列出某保護區的物種代碼（標示可出巨獸的物種）
cotwmod pop show hirsch               # 目前的數量、公母、鑽石、巨獸、稀有毛色統計
cotwmod pop apply my_profile.yaml --dry-run   # 試跑，不寫入存檔
cotwmod pop apply my_profile.yaml     # 套用（自動備份）
cotwmod pop restore hirsch            # 還原到第一次修改前的原始存檔
```

配置檔範例 [config/population_profile.example.yaml](config/population_profile.example.yaml)：

```yaml
reserves:
  hirsch:
    red_deer:
      males: "+20"          # +N 增加 / -N 減少 / =N 設定為 N 隻
      to_males: 10%         # 10% 母鹿轉為公鹿
      rare_furs: 15%        # 稀有毛色
      diamonds: 8           # 鑽石：數量、百分比或 all
      diamond_rare_furs: true
      great_ones: 2         # 巨獸
```

執行順序固定為：數量 → 性別 → 稀有毛色 → 鑽石 → 巨獸，後面的步驟不會被前面覆蓋。
備份位置：`%USERPROFILE%\.cotwmod\backups\`（`original\` 是第一次修改前的原檔，永不覆蓋）。

已知限制（繼承自遊戲/APC）：每個動物群組最多 30 隻；使用 TruRACS 新評分系統的部分物種鑽石生成可能不穩定；新地圖需等 APC 更新物種資料（`git submodule update --remote`）。

---

## 專案結構

```
cotwmod/
  ballistics/solver.py       彈道求解（RK4、阻力、風、坡度、歸零）
  ballistics/projection.py   角度 ↔ 螢幕像素（FOV、倍率）
  ballistics/calibration.py  Levenberg–Marquardt 參數擬合
  game/memory.py             唯讀記憶體讀取（指標鏈 / AOB）
  game/aim.py                合併手動與實時輸入，計算落點
  overlay/overlay.py         透明疊加層 + 全局熱鍵
  population/profile.py      種群配置檔解析與驗證
  population/apply.py        基於 APC 套用、備份、還原
config/                      武器、設定、位址、校準結果
third_party/apc              APC（子模組）
tests/                       pytest
```

開發：`pip install -e .[dev]` 然後 `pytest`。

## 授權

MIT。`third_party/apc` 屬於其原作者（MIT）。
