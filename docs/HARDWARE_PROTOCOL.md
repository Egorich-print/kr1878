# HARDWARE_PROTOCOL.md — Аппаратный протокол программатора/отладки

> Восстановлено по дизассемблированию `E03_PROG.EXE` и `P_WMCT.EXE`, даташит гл. 08–09.
> Цель: дать точный протокол для реализации `ProgrammerBackend` в Rust.

---

## 1. Интерфейсы оборудования (два НЕЗАВИСИМЫХ пути)

| Инструмент | Интерфейс | Адреса | Роль |
|------------|-----------|--------|------|
| **Программатор** (E03_PROG / P_WMCT) | **LPT1 (parallel)** | data `0x378`, status `0x379`, control `0x37A` | прошивка ЭСППЗУ команд + данных |
| **Отладочный модуль** (MC_WIN) | **COM (serial UART)** | COM1/COM2 (база на runtime) | встраиваемый отладчик/эмулятор |

### Распиновка LPT-битбанга (программатор)

Регистр **CONTROL (0x37A)** — тактируемые выходы кабеля:
| Бит control | Сигнала кабеля (из дизассемблера) | Функция |
|-------------|--------|----------|
| C0 (bit0) | STROBE/# | данные (передач) |
| C1 (bit1) | AutoFeed | тактовый (передач) |
| C2 (bit2) | INIT | тактовый (приём) |
| C3 (bit3) | SelectIn | командный/эраз импульс |

Регистр **STATUS (0x379)** — входы:
| Бит | Линия | Функция |
|-----|-------|---------|
| bit7 (инвертирован) | BUSY | данные (приём, `ReadBit7`) |
| bit6 | ACK | handshake готовности |

Регистр **DATA (0x378)** — командный байт-фазы: `0x38` / `0x3F` / `0x00`.

> Линия `BUSY` на выходе LPT конструктивно инвертируется (spike); декодер применяет инверсию (`in al,0x379; not al`).

---

## 2. Низкоуровневые примитивы (эквивалент Rust)

### 2.1 `OutCtrl(mask)` — запись комбинации control-битов в 0x37A
Параллельно кодированию по маске (C0..C3), с ресем инверсией C2 по умолчанию. (Детальный флаг-мативинг в RESERVER алге: overhead высчит-вовый.)

### 2.2 `ReadBit7()` / `ReadBit6()`
```rust
fn read_busy() -> bool { !(in_status(STATUS) >> 7) & 1 }  // 0x379 hbit7 инв.
fn read_ack()  -> bool { !(in_status(STATUS) >> 6) & 1 }
```

### 2.3 `WaitForAck(expected: bool)`
- Пульс ACK (status bit6) до равенства `expected`; либо до **10000 итераций** → «Microcontroller Not Ready».

### 2.4 `WriteBits(n, value)` — передача `n` бит MSB-first
```
1. idle: control ← 0x0F
2. Ждём ACK=1 (ждутся)
3. control &= ~1 ; control |= 1-й бит данных   // данные → C0
4. control ^= ~2 ; |2 ; control |= 2          // такт C1 (низ→выс)
5. битовой сдвиг value <<= 1; повторяя для всех n бит старшим вперёд
6. control ← 0x0F (idle)
```

### 2.5 `ReadBits(n)` — приём n бит
```
1. control ← 0x0F
2. wait ack high
3. data |= read_busy() << позиция;  value>>=1
4. control &= ~0x08 (trigger C3)       // find also - see
5. wait ack low
6. control ← 0x0F
   n == 8 → value >>= 8 (сдвиг байт)
```

### 2.6 Командный/эразовый импульс
```
control ← 0x0F
DATA(0x378) ← 0x38
loop 20× { control&~0x08; control|0x08 }   // toggle C3
DATA(0x378) ← 0x3F
```

### 2.7 Idle / off
```
control ← 0; DATA(0x378) ← 0x00
```

---

## 3. Протокол прошивания МК (алгоритм)

Выполняется для Program + Config + Data (ЭСПЗУ команд и данных):

```
1.  idle (control=0, DATA=0)
2.  ERASE  : командный импульс 0x38/0x3F, 20 toggle; WaitForAck
3.  WriteCommand: CMDWrite (командный код)            # команда = 0x03 для программatorа, 0x02 для данных
4.  WRITE program memory:
       for word in program_words (или for byte in data_bytes):
           WaitForAck                              # MC не готов пока занят
           WriteBits(16, word)                     # или WriteBits(8, byte)
5.  VERIFY (read-back):
       for each:
           value = ReadBits(16) (или ReadBits(8))
           if value != expected: "Programm Read Error" (или "Data Read Error") + трасса Address/Pattern/Data
```

Последовательность по состояниям диспетчера (E03_PROG):
1. Erase
2. ждать окончания / timeout → «Microcontroller Not Ready»/«Error»
3. CMDWrite (program) = код `0x03`
4. запись памяти программ (слова)
5. **Verify памяти программ** (ReadBits16)
6. CMDWrite (data) = код `0x02`
7. запись ЭСПРЗУ данных (байты)
8. **Verify данных** (ReadBits8)

`OSC_Configuration` — отдельный канал (ReadConfig), использует поле `0x4062BC`.

> `ERASE` из инспектируемого варианта используется отдельно (состояния 0–1), расширение — статус вариант в команде перед записью.

---

## 4. Абстракция для Rust

```rust
pub trait ProgrammerBackend {
    fn identify(&mut self) -> Result<DeviceId, Error>;
    fn erase(&mut self) -> Result<(), Error>;
    fn write_program(&mut self, words: &[u16]) -> Result<(), Error>;   // 16 бит
    fn write_data(&mut self, bytes: &[u8; 64]) -> Result<(), Error>;   // 8 бит
    fn read_program(&mut self) -> Result<Vec<u16>, Error>;
    fn read_data(&mut self) -> Result<[u8; 64], Error>;
    fn verify_program(&mut self, words: &[u16]) -> Result<bool, Error>;
    fn verify_data(&mut self, bytes: &[u8; 64]) -> Result<bool, Error>;
}
```

Реализации:
- `LptBackend` — прямое обращение к портам (Windows: `inpout32`/`inpoutx64`; Linux: `ppdev`/sysfs; macOS: недоступен без драйвера) — сохраняет совместимость с оригиналом.
- `UsbBridgeBackend` — современный USB-мост, работающий по тем же командам (простейший бейсик-исполнитель команд; логика — в Rust Core).
- `MockBackend` / `SimulatorBackend` — для юнит/интеграционных тестов и CI без железа.

---

## 5. Порядок инициализации питания МК

(из даташита, гл. 08, таблица 10)
- Программатор/модуль должен установить RST=0 (низ.), подать питание UCC ≥ 3.5V, затем RST→1 и выждать задержку начального запуска (счётчик 2^10 импульсов тактовой частоты). Это требуется для управляемого сброса и запуска — используется в reset delay (бит0 в `/c:NNN` P_WMCT).

---

## 6. COM-протокол отладочного модуля `[GAP]`

- Соединение COM1/COM2 почтовым UART, raw I/O.
- Команды: инициализация («Test Envelope» диагностика возврат ID/test-байт), `PC=0` сброс PC, загрузка `*.SAV` в память команд, чтение/запись ОЗУ/регистров, точки осненваново (SetBRK, до 4).
- **Точный байт-протокол не восстановлен статически** [GAP] — требует полного дизассемблирования MC_WIN или логического анализатора на реальном модуле. Этап отладки (часть 12) переносится в приоритет после заявленного core (прошивание) — фикс. см. ADR.