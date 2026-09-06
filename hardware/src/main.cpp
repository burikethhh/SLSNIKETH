/*
  GymPOS Access Controller — ESP32 Firmware
  ──────────────────────────────────────────
  Receives commands via USB Serial from the GymPOS Desktop application.
  Controls door solenoid/relay, buzzer, and 16x2 I2C LCD.
  RFID is handled by external USB reader on the PC side.

  Pin Wiring (Field Connections):
    GPIO4   → LCD SDA           (software I2C, Wire.begin(4, 5))
    GPIO5   → LCD SCL           (software I2C)
    GPIO10  → Buzzer            (active HIGH, other lead to GND)
    5V      → LCD VCC
    GND     → Common Ground (LCD + Buzzer)

  Hardware Notes:
    - LCD is a standard 4-pin I2C backpack: SDA, SCL, GND, and 5V only (no LED pin).
    - Status feedback is rendered directly on the 16x2 LCD using custom status icons:
        Character 0: Lock icon
        Character 1: Unlock icon
        Character 2: Alert icon
    - Idle Screen:
        Line 1: [Lock] GymPOS by SLS
        Line 2:    Scan Face
    - Verified Entry:
        Line 1: [Unlock] Welcome
        Line 2: <Member Name>
    - Verified Exit:
        Line 1: [Unlock] Bye
        Line 2: <Member Name>
    - Continuous camera feeds with automated face detection & anti-tailgate
      verification handled upstream by the GymPOS PC host application.
    - Tailgate alarm arms automatically on every verified entry/exit.

  Relay Control:
    - The onboard relay's driver input is broken out on the header as "Rel".
      Jumper Rel → GPIO3 (LOCK_PIN): relay OFF (locked) = pin floating/INPUT,
      relay ON (unlocked) = pin OUTPUT+LOW (active-LOW driver, matches the
      relay_direct test convention). Wire the maglock through the COM/NO
      screw terminal.
    - Override at build time with -D LOCK_PIN=<gpio> (-1 = logical only).

  Serial Commands (case-insensitive command names, \n terminated):
    WELCOME:<name>[|<sec>] → unlock + success beep + LCD "Welcome" / "<name>"
    BYE:<name>[|<sec>]     → unlock + exit beep + LCD "Bye" / "<name>"
    UNLOCK                 → open solenoid 5s + success beep + LCD "Access Granted"
    UNLOCK:<seconds>       → open solenoid for custom duration (in seconds or ms)
    LOCK                   → force-lock immediately + LCD "Door Locked"
    DENY                   → triple beep + LCD "Access Denied"
    DENY:<reason>          → triple beep + LCD shows reason text
    BEEP                   → single short beep
    BEEP_LONG              → long 2-second warning beep
    ALERT_TAILGATE         → rapid pulsing alarm (5s default) + LCD warning
    ALERT_TAILGATE:<ms>    → pulsing alarm for a custom duration (1s..15s)
    ALARM                  → alias for ALERT_TAILGATE
    IDLE:<line1>|<line2>   → owner-branded idle screen, persisted in NVS
    LCD:<line1>|<line2>    → display custom text on LCD (| separates lines)
    LCD_CLEAR              → clear LCD and return to idle screen
    LCD_REINIT             → re-probe the LCD on SDA=4/SCL=5 and re-init
    FINDRELAY              → diagnostic: cycle candidate GPIOs, LCD shows which
    STOPRELAY              → stop the relay finder
    PING                   → reply ACK:PONG (health check)
    STATUS                 → reply with current lock state (ACK:STATUS:LOCKED|UNLOCKED)
    VERSION                → reply ACK:VERSION:<fw-version>

  Messages Sent TO PC:
    READY:fw=<version>     → sent once on boot
    ACK:<cmd>              → acknowledgement for every command
    ACK:RELOCK             → auto-lock notification after unlock timer expires

  Watchdog:
    The loop task is watched by the ESP32 task watchdog (10s). An I2C/LCD
    lockup can never brick the door controller — the board reboots fail-SECURE
    (locked), because unlock state is never persisted across a reboot.
*/

#include <Arduino.h>
#include <Wire.h>
#include <LiquidCrystal_I2C.h>
#include <Preferences.h>
#include <esp_task_wdt.h>

// ───────────── Firmware Version ──────────────
// 1.0.0  — original direct-wired relay build
// 1.1.0  — buzzer moved off the GPIO9 strapping pin, VERSION command,
//          ALERT_TAILGATE:<ms>, task watchdog
// 1.1.1  — fixed-field-wiring LCD probe (SDA=5/SCL=4): removed the all-pins
//          auto-scanner that drove the buzzer line and the USB pads (18/19)
// 1.1.2  — LCD probe tolerates a swapped SDA/SCL orientation
// 1.2.0  — owner-branded idle screen (IDLE:<l1>|<l2>, NVS-persisted)
// 1.2.1  — FINDRELAY/STOPRELAY diagnostic: identify the onboard relay's
//          driver GPIO by ear (LCD shows the live candidate)
static const char* FW_VERSION = "1.2.1";

// ───────────── Lock Configuration ─────────────
// All locks are direct-wired to board relay COM/NC (no ESP GPIO pin needed).
// If an active GPIO pin is wired to a relay module, define LOCK_PIN >= 0.
#ifndef LOCK_PIN
#define LOCK_PIN -1  // -1 = Direct-wired relay / logical tracking only
#endif

// ───────────── Pin Configuration ─────────────
// Field wiring (verified): 4-pin I2C LCD backpack — SDA=GPIO4, SCL=GPIO5.
// The boot-time probe also tolerates a swapped SDA/SCL orientation.
#ifndef LCD_SDA_PIN
static const int LCD_SDA_PIN    = 4;   // I2C SDA (Wire.begin(4, 5))
#endif
#ifndef LCD_SCL_PIN
static const int LCD_SCL_PIN    = 5;   // I2C SCL
#endif
#ifndef BUZZER_PIN
static const int BUZZER_PIN     = 10;  // active buzzer on safe GPIO10 (NOT GPIO9 strapping pin)
#endif

// ───────────── I2C LCD Probe (fixed field wiring) ─────────────
// Field wiring is FIXED: SDA=GPIO5, SCL=GPIO4, buzzer=GPIO10. The old
// auto-scanner that swept every pin pair is gone for good reason — it drove
// the buzzer line as I2C (squeal) and tickled the C3's USB pins GPIO18/19,
// ACKing a phantom "LCD" at 0x3F and re-routing all LCD traffic to the USB
// pads. We now probe only the known pins for the two common addresses; if
// nothing ACKs, the LCD object is still created so commands stay safe.
static const uint8_t LCD_CANDIDATE_ADDRS[] = {0x27, 0x3F, 0x26, 0x38, 0x20};

struct I2cResult {
  int sda;
  int scl;
  uint8_t addr;
};

int activeSdaPin = LCD_SDA_PIN;
int activeSclPin = LCD_SCL_PIN;

// Probe the fixed field pins for a backpack, trying the documented
// orientation (SDA=5/SCL=4) first and then the swapped one — a swapped
// SDA/SCL otherwise never ACKs and looks exactly like a missing panel.
static I2cResult probeLcdOnFixedPins() {
  const int orientations[][2] = {
    {LCD_SDA_PIN, LCD_SCL_PIN},   // documented: SDA=4, SCL=5
    {LCD_SCL_PIN, LCD_SDA_PIN},   // swapped tolerance
  };
  for (auto& o : orientations) {
    Wire.end();
    Wire.setPins(o[0], o[1]);
    Wire.begin(o[0], o[1]);
    Wire.setTimeOut(50);
    Wire.setClock(100000);
    for (uint8_t a : LCD_CANDIDATE_ADDRS) {
      Wire.beginTransmission(a);
      if (Wire.endTransmission() == 0) {
        activeSdaPin = o[0];
        activeSclPin = o[1];
        Serial.printf("  LCD found on SDA=GPIO%d, SCL=GPIO%d, Addr=0x%02X\n", o[0], o[1], a);
        return {o[0], o[1], a};
      }
    }
  }
  Serial.printf("  No LCD ACKed on SDA=%d/SCL=%d (or swapped) — defaulting to 0x27 (check wiring/power)\n",
                LCD_SDA_PIN, LCD_SCL_PIN);  return {LCD_SDA_PIN, LCD_SCL_PIN, 0x27};
}

// ───────────── Peripherals ───────────────────
LiquidCrystal_I2C* lcd = nullptr;     // Dynamically created after I2C address detection

// ───────────── Timing State ──────────────────
static const unsigned long DEFAULT_UNLOCK_MS = 5000;

unsigned long unlockUntil   = 0;
unsigned long lcdIdleAfter  = 0;       // auto-return to idle screen
bool isLocked               = true;

// ───────────── Custom LCD Characters ─────────
// Lock icon (Character 0)
byte lockChar[8]   = {0b01110,0b10001,0b10001,0b11111,0b11011,0b11011,0b11111,0b00000};
// Unlock icon (Character 1)
byte unlockChar[8] = {0b01110,0b10000,0b10000,0b11111,0b11011,0b11011,0b11111,0b00000};
// Alert icon (Character 2)
byte alertChar[8]  = {0b00100,0b00100,0b01110,0b01110,0b11111,0b11111,0b00100,0b00000};

// ───────────── Branded Idle Screen ───────────
// Line 1 is the owner's brand (set by the GymPOS exe via IDLE:<l1>|<l2> and
// persisted in NVS so it survives power cycles); line 2 is the call to action.
char idleLine1[17] = "GymPOS by SLS";
char idleLine2[17] = "   Scan Face";

// ───────────── Non-blocking Buzzer ───────────
// Plays patterns without delay() so the main loop keeps running smoothly.
// Pattern = array of durations: positive = buzzer ON, negative = silence, 0 = end.
static const int PAT_SUCCESS[]     = {120, -100, 120, 0};          // 2 equal short beeps (granted)
static const int PAT_DENY[]        = {100, -80, 100, -80, 100, 0}; // triple beep (denied)
static const int PAT_EXIT[]        = {80, -60, 80, 0};             // two short beeps (exit)
static const int PAT_STARTUP[]     = {60, -40, 80, -40, 100, 0};   // ascending chirp (boot)
static const int PAT_BEEP[]        = {150, 0};                     // single short beep
static const int PAT_BEEP_LONG[]   = {2000, 0};                    // single long 2-sec warning beep

const int* buzzerPattern  = nullptr;
int        buzzerStep     = 0;
unsigned long buzzerUntil = 0;

void buzzerStart(const int* pattern) {
  buzzerPattern = pattern;
  buzzerStep = 0;
  int dur = pattern[0];
  if (dur > 0) {
    digitalWrite(BUZZER_PIN, HIGH);
    buzzerUntil = millis() + (unsigned long)dur;
  } else if (dur < 0) {
    digitalWrite(BUZZER_PIN, LOW);
    buzzerUntil = millis() + (unsigned long)(-dur);
  }
}

void buzzerTick() {
  if (!buzzerPattern) return;
  if ((long)(millis() - buzzerUntil) < 0) return;  // overflow-safe check

  buzzerStep++;
  int dur = buzzerPattern[buzzerStep];
  if (dur == 0) {
    // Pattern complete
    digitalWrite(BUZZER_PIN, LOW);
    buzzerPattern = nullptr;
    return;
  }
  if (dur > 0) {
    digitalWrite(BUZZER_PIN, HIGH);
    buzzerUntil = millis() + (unsigned long)dur;
  } else {
    digitalWrite(BUZZER_PIN, LOW);
    buzzerUntil = millis() + (unsigned long)(-dur);
  }
}

// ───────────── Tailgate Alarm ────────────────
// Same 200ms-on/100ms-off pulse shape as the old fixed pattern, generated
// dynamically for the requested duration so the host (whose siren length is
// policy-controlled from the cloud) can size the alarm.
static int dynAlertPattern[(15000 / 300) + 2];  // worst case: 50 pulses + terminator

void startTailgateAlarm(unsigned long ms) {
  const unsigned long stepOn = 200, stepOff = 100;
  const int maxSteps = (int)(sizeof(dynAlertPattern) / sizeof(dynAlertPattern[0])) - 1;
  int n = 0;
  unsigned long total = 0;
  while (total + stepOn + stepOff <= ms && n + 2 <= maxSteps) {
    dynAlertPattern[n++] = (int)stepOn;
    dynAlertPattern[n++] = -(int)stepOff;
    total += stepOn + stepOff;
  }
  if (total < ms && n < maxSteps) {  // trailing partial pulse
    dynAlertPattern[n++] = (int)(ms - total);
  }
  dynAlertPattern[n] = 0;
  buzzerStart(dynAlertPattern);
}

// ───────────── Watchdog ──────────────────────
// The loop task is watched by the ESP32 task watchdog: an I2C/LCD lockup
// reboots the board instead of bricking the door controller. Reboot state is
// fail-SECURE — boot always starts locked.
static const uint32_t WDT_TIMEOUT_MS = 10000;

void initWatchdog() {
#if defined(ESP_ARDUINO_VERSION_MAJOR) && (ESP_ARDUINO_VERSION_MAJOR >= 3)
  esp_task_wdt_config_t cfg = {};
  cfg.timeout_ms = WDT_TIMEOUT_MS;
  cfg.idle_core_mask = 0;
  cfg.trigger_panic = true;
  if (esp_task_wdt_init(&cfg) == ESP_ERR_INVALID_STATE) {
    esp_task_wdt_reconfigure(&cfg);  // core already initialized it — adjust timeout
  }
#else
  esp_task_wdt_init((WDT_TIMEOUT_MS + 999) / 1000, true);
#endif
  esp_task_wdt_add(NULL);  // watch the current (loopTask) task
}

// ───────────── LCD Helper Prototypes ─────────
void lcdShowIdle();
void lcdShow(const String& line1, const String& line2, unsigned long autoIdleMs);

// ───────────── Relay Pin Identifier ──────────
// One-shot field diagnostic: cycles candidate GPIOs (OUTPUT+LOW 1.2s = relay
// ON per the active-LOW convention, then high-Z = relay OFF) and shows the
// current candidate on the LCD, so the onboard relay's driver pin can be
// identified by ear. STOPRELAY ends it; LOCK/UNLOCK/WELCOME/BYE also cancel.
bool findRelayMode = false;
int  findRelayIdx = 0;
unsigned long findRelayUntil = 0;
bool findRelayActive = false;
static const int RELAY_CANDIDATES[] = {4, 0, 1, 2};

void stopRelayFinder() {
  if (!findRelayMode) return;
  findRelayMode = false;
  for (int p : RELAY_CANDIDATES) pinMode(p, INPUT);  // release all candidates
  lcdShowIdle();
}

void startRelayFinder() {
  findRelayMode = true;
  findRelayIdx = 0;
  findRelayActive = false;
  findRelayUntil = millis();  // tick immediately
  Serial.println("ACK:FINDRELAY");
}

void findRelayTick() {
  if (!findRelayMode) return;
  unsigned long now = millis();
  if ((long)(now - findRelayUntil) < 0) return;
  findRelayActive = !findRelayActive;
  int pin = RELAY_CANDIDATES[findRelayIdx];
  char l1[17], l2[17];
  snprintf(l1, sizeof(l1), "RELAY? GPIO%d", pin);
  if (findRelayActive) {
    pinMode(pin, OUTPUT);
    digitalWrite(pin, LOW);   // active-LOW: relay energizes (CLICK)
    snprintf(l2, sizeof(l2), ">> ON — listen");
    findRelayUntil = now + 1200;
  } else {
    pinMode(pin, INPUT);      // high-Z: relay OFF
    snprintf(l2, sizeof(l2), "off");
    findRelayUntil = now + 1300;
    findRelayIdx = (findRelayIdx + 1) % (int)(sizeof(RELAY_CANDIDATES) / sizeof(RELAY_CANDIDATES[0]));
  }
  lcdShow(l1, l2, 0);
}

// ───────────── LCD Helpers ───────────────────
void lcdShowIdle();

void initLcdHardware(int sda, int scl, uint8_t addr) {
  activeSdaPin = sda;
  activeSclPin = scl;
  if (lcd) {
    delete lcd;
    lcd = nullptr;
  }
  Wire.end();
  Wire.setPins(activeSdaPin, activeSclPin);
  Wire.begin(activeSdaPin, activeSclPin);
  Wire.setTimeOut(50);
  Wire.setClock(100000);

  lcd = new LiquidCrystal_I2C(addr, 16, 2);
  // init() executes full HD44780 4-bit initialization
  Wire.end();
  Wire.setPins(activeSdaPin, activeSclPin);
  Wire.begin(activeSdaPin, activeSclPin);
  lcd->init();
  Wire.setTimeOut(50);
  Wire.setClock(100000);

  lcd->backlight();
  delay(20);
  lcd->clear();
  delay(20);
  lcd->createChar(0, lockChar);
  lcd->createChar(1, unlockChar);
  lcd->createChar(2, alertChar);
  lcdShowIdle();
}

void lcdShowIdle() {
  if (!lcd) return;
  lcd->clear();
  lcd->setCursor(0, 0);
  lcd->write(0);  // lock icon
  lcd->print(" ");
  lcd->print(idleLine1);
  lcd->setCursor(0, 1);
  lcd->print(idleLine2);
}

void setIdleScreen(String line1, String line2) {
  line1.replace("|", " ");
  line1.replace(':', ' ');
  line1.trim();
  line2.replace("|", " ");
  line2.replace(':', ' ');
  line2.trim();
  if (line1.length() == 0) line1 = "GymPOS by SLS";
  if (line2.length() == 0) line2 = "Scan Face";
  line1.toCharArray(idleLine1, sizeof(idleLine1));
  line2.toCharArray(idleLine2, sizeof(idleLine2));
  // Persist so the brand survives power cycles without the exe re-sending it.
  Preferences prefs;
  prefs.begin("gympos", false);
  prefs.putString("idle1", idleLine1);
  prefs.putString("idle2", idleLine2);
  prefs.end();
  if (lcd) lcdShowIdle();
}

void lcdShow(const String& line1, const String& line2, unsigned long autoIdleMs = 5000) {
  if (!lcd) return;
  lcd->clear();
  lcd->setCursor(0, 0);
  lcd->print(line1.substring(0, 16));
  if (line2.length() > 0) {
    lcd->setCursor(0, 1);
    lcd->print(line2.substring(0, 16));
  }
  if (autoIdleMs > 0) {
    lcdIdleAfter = millis() + autoIdleMs;
  } else {
    lcdIdleAfter = 0;
  }
}

// ───────────── Lock / Unlock ─────────────────
void setLocked(bool locked) {
  isLocked = locked;
#if defined(LOCK_PIN) && (LOCK_PIN >= 0)
  if (locked) {
    pinMode(LOCK_PIN, INPUT);      // high impedance → relay OFF (locked)
  } else {
    pinMode(LOCK_PIN, OUTPUT);
    digitalWrite(LOCK_PIN, LOW);   // active LOW pulls relay IN to GND (unlocked)
  }
#endif
}

void unlockDoor(unsigned long ms) {
  unlockUntil = millis() + ms;
  setLocked(false);
  buzzerStart(PAT_SUCCESS);
  if (lcd) {
    lcd->clear();
    lcd->setCursor(0, 0);
    lcd->write(1);  // unlock icon
    lcd->print(" ACCESS GRANTED");
    lcd->setCursor(0, 1);
    lcd->print("Door open ");
    lcd->print(ms / 1000);
    lcd->print("s");
  }
  lcdIdleAfter = millis() + ms + 1000;
  Serial.println("ACK:UNLOCK");
}

void grantWelcome(const String& name, unsigned long ms) {
  unlockUntil = millis() + ms;
  setLocked(false);
  buzzerStart(PAT_SUCCESS);
  if (lcd) {
    lcd->clear();
    lcd->setCursor(0, 0);
    lcd->write(1);  // unlock icon
    lcd->print(" Welcome");
    lcd->setCursor(0, 1);
    String displayName = (name.length() > 0) ? name : "Member";
    lcd->print(displayName.substring(0, 16));
  }
  lcdIdleAfter = millis() + ms + 1500;
  Serial.println("ACK:WELCOME");
}

void grantBye(const String& name, unsigned long ms) {
  unlockUntil = millis() + ms;
  setLocked(false);
  buzzerStart(PAT_EXIT);
  if (lcd) {
    lcd->clear();
    lcd->setCursor(0, 0);
    lcd->write(1);  // unlock icon
    lcd->print(" Bye");
    lcd->setCursor(0, 1);
    String displayName = (name.length() > 0) ? name : "Member";
    lcd->print(displayName.substring(0, 16));
  }
  lcdIdleAfter = millis() + ms + 1500;
  Serial.println("ACK:BYE");
}

// ───────────── Helpers ───────────────────────
unsigned long parseSeconds(const String& arg, unsigned long defaultMs) {
  // Parse "10" → 10000ms, "3000" → 3000ms, fallback to defaultMs
  if (arg.length() == 0) return defaultMs;
  long val = arg.toInt();
  if (val <= 0) return defaultMs;
  if (val > 300) {
    // Value is already in milliseconds (e.g. 3000ms), cap at 5 minutes
    return (val > 300000) ? 300000UL : (unsigned long)val;
  }
  // Value is in seconds (e.g. 5s → 5000ms)
  return (unsigned long)val * 1000UL;
}

// ───────────── Command Parser ────────────────
void processSerialCommand(const String& cmdRaw) {
  String trimmed = cmdRaw;
  trimmed.trim();
  if (trimmed.length() == 0) return;

  // Split on first ':' for parameterized commands
  int colonIdx = trimmed.indexOf(':');
  String base = (colonIdx >= 0) ? trimmed.substring(0, colonIdx) : trimmed;
  base.trim();
  base.toUpperCase();

  String argRaw = (colonIdx >= 0) ? trimmed.substring(colonIdx + 1) : "";
  argRaw.trim();

  // ── WELCOME ── (WELCOME:John Doe or WELCOME:John Doe|3)
  if (base == "WELCOME") {
    int pipeIdx = argRaw.indexOf('|');
    String name = (pipeIdx >= 0) ? argRaw.substring(0, pipeIdx) : argRaw;
    name.trim();
    unsigned long ms = (pipeIdx >= 0) ? parseSeconds(argRaw.substring(pipeIdx + 1), DEFAULT_UNLOCK_MS) : DEFAULT_UNLOCK_MS;
    grantWelcome(name, ms);
    return;
  }

  // ── BYE ── (BYE:John Doe or BYE:John Doe|3)
  if (base == "BYE" || base == "GOODBYE") {
    int pipeIdx = argRaw.indexOf('|');
    String name = (pipeIdx >= 0) ? argRaw.substring(0, pipeIdx) : argRaw;
    name.trim();
    unsigned long ms = (pipeIdx >= 0) ? parseSeconds(argRaw.substring(pipeIdx + 1), DEFAULT_UNLOCK_MS) : DEFAULT_UNLOCK_MS;
    grantBye(name, ms);
    return;
  }

  // ── UNLOCK ──
  if (base == "UNLOCK") {
    String argUpper = argRaw;
    argUpper.toUpperCase();
    if (argUpper.startsWith("IN:") || argUpper.startsWith("WELCOME:")) {
      String name = argRaw.substring(argRaw.indexOf(':') + 1);
      grantWelcome(name, DEFAULT_UNLOCK_MS);
      return;
    }
    if (argUpper.startsWith("OUT:") || argUpper.startsWith("BYE:")) {
      String name = argRaw.substring(argRaw.indexOf(':') + 1);
      grantBye(name, DEFAULT_UNLOCK_MS);
      return;
    }
    unsigned long ms = parseSeconds(argRaw, DEFAULT_UNLOCK_MS);
    unlockDoor(ms);
    return;
  }

  // ── LOCK ──
  if (base == "LOCK") {
    unlockUntil = 0;
    setLocked(true);
    lcdShow("\x00 DOOR LOCKED", "", 3000);
    Serial.println("ACK:LOCK");
    return;
  }

  // ── DENY ──
  if (base == "DENY") {
    buzzerStart(PAT_DENY);
    if (lcd) {
      lcd->clear();
      lcd->setCursor(0, 0);
      lcd->print("  ACCESS DENIED");
      lcd->setCursor(0, 1);
      if (argRaw.length() > 0) {
        lcd->print(argRaw.substring(0, 16));
      } else {
        lcd->print("  Unauthorized");
      }
    }
    lcdIdleAfter = millis() + 3000;
    Serial.println("ACK:DENY");
    return;
  }

  // ── BEEP / BEEP_LONG ──
  if (base == "BEEP_LONG") {
    buzzerStart(PAT_BEEP_LONG);
    Serial.println("ACK:BEEP_LONG");
    return;
  }
  if (base == "BEEP") {
    buzzerStart(PAT_BEEP);
    Serial.println("ACK:BEEP");
    return;
  }

  // ── ALERT_TAILGATE / ALARM ── (rapid buzzer pulse; 5s default, optional ms)
  if (base == "ALERT_TAILGATE" || base == "ALARM") {
    unsigned long ms = parseSeconds(argRaw, 5000UL);
    if (ms < 1000) ms = 1000;
    if (ms > 15000) ms = 15000;
    startTailgateAlarm(ms);
    if (lcd) {
      lcd->clear();
      lcd->setCursor(0, 0);
      lcd->write(2);  // alert icon
      lcd->print(" TAILGATE ALERT");
      lcd->setCursor(0, 1);
      lcd->print("Multiple entries!");
    }
    lcdIdleAfter = millis() + ms;
    Serial.println("ACK:ALERT_TAILGATE");
    return;
  }

  // ── LCD ── (LCD:Hello World|Line 2)
  if (base == "LCD") {
    int pipe = argRaw.indexOf('|');
    String l1 = (pipe >= 0) ? argRaw.substring(0, pipe) : argRaw;
    String l2 = (pipe >= 0) ? argRaw.substring(pipe + 1) : "";
    lcdShow(l1, l2, 8000);
    Serial.println("ACK:LCD");
    return;
  }

  // ── LCD_CLEAR ──
  if (base == "LCD_CLEAR") {
    lcdShowIdle();
    Serial.println("ACK:LCD_CLEAR");
    return;
  }

  // ── LCD_REINIT ──
  if (base == "LCD_REINIT") {
    I2cResult res = probeLcdOnFixedPins();
    initLcdHardware(res.sda, res.scl, res.addr);
    Serial.printf("ACK:LCD_REINIT:SDA=%d:SCL=%d:ADDR=0x%02X\n", res.sda, res.scl, res.addr);
    return;
  }

  // ── FINDRELAY / STOPRELAY ── (field diagnostic: identify the onboard
  // relay's driver GPIO by ear — the LCD shows which candidate is live)
  if (base == "FINDRELAY") {
    startRelayFinder();
    return;
  }
  if (base == "STOPRELAY") {
    stopRelayFinder();
    Serial.println("ACK:STOPRELAY");
    return;
  }

  // ── IDLE ── (IDLE:<brand>|Scan Face — owner-branded idle screen, NVS-persisted)
  if (base == "IDLE") {
    int pipe = argRaw.indexOf('|');
    String l1 = (pipe >= 0) ? argRaw.substring(0, pipe) : argRaw;
    String l2 = (pipe >= 0) ? argRaw.substring(pipe + 1) : "";
    setIdleScreen(l1, l2);
    Serial.println("ACK:IDLE");
    return;
  }

  // ── PING ──
  if (base == "PING") {
    Serial.println("ACK:PONG");
    return;
  }

  // ── VERSION ──
  if (base == "VERSION") {
    Serial.printf("ACK:VERSION:%s\n", FW_VERSION);
    return;
  }

  // ── STATUS ──
  if (base == "STATUS") {
    Serial.print("ACK:STATUS:");
    Serial.println(isLocked ? "LOCKED" : "UNLOCKED");
    return;
  }

  Serial.print("ACK:UNKNOWN:");
  Serial.println(trimmed);
}

// ───────────── Setup ─────────────────────────
void setup() {
  // Pin modes
  pinMode(BUZZER_PIN, OUTPUT);
  digitalWrite(BUZZER_PIN, LOW);
  setLocked(true);

  // Serial interface
  Serial.begin(115200);
  delay(100);
  Serial.setTimeout(50);
  Serial.printf("\nGymPOS Controller v%s booting...\n", FW_VERSION);

  // Load the owner-branded idle screen from NVS (defaults if never set).
  Preferences prefs;
  prefs.begin("gympos", true);
  String idle1 = prefs.getString("idle1", "GymPOS by SLS");
  String idle2 = prefs.getString("idle2", "Scan Face");
  prefs.end();
  idle1.toCharArray(idleLine1, sizeof(idleLine1));
  idle2.toCharArray(idleLine2, sizeof(idleLine2));

  // Probe the I2C LCD on the fixed field wiring (SDA=4/SCL=5, swap-tolerant).
  // NOTE: this runs BEFORE the startup chirp — the probe blocks the loop (no
  // buzzerTick), so any pattern started here would hold the buzzer solid
  // until it finishes.
  I2cResult res = probeLcdOnFixedPins();
  initLcdHardware(res.sda, res.scl, res.addr);

  // Startup chirp plays cleanly now: loop() starts right after and drives
  // the rest of the pattern via buzzerTick().
  buzzerStart(PAT_STARTUP);

  // Watchdog goes live LAST: the boot-time I2C pin sweep can legitimately
  // take >10s with no LCD connected, and the steady-state loop is what the
  // watchdog exists to protect.
  initWatchdog();

  Serial.printf("READY:fw=%s\n", FW_VERSION);
}

// ───────────── Main Loop ─────────────────────
void loop() {
  unsigned long now = millis();

  // ── Watchdog feed ──
  esp_task_wdt_reset();

  // ── Non-blocking buzzer tick ──
  buzzerTick();

  // ── Relay-pin finder (field diagnostic) ──
  findRelayTick();

  // ── Auto-relock after unlock timer ──
  if (unlockUntil > 0 && (long)(now - unlockUntil) >= 0) {
    unlockUntil = 0;
    setLocked(true);
    Serial.println("ACK:RELOCK");
  }

  // ── LCD auto-return to idle ──
  if (lcdIdleAfter > 0 && (long)(now - lcdIdleAfter) >= 0) {
    lcdIdleAfter = 0;
    lcdShowIdle();
  }

  // ── Serial command processing ──
  if (Serial.available()) {
    String line = Serial.readStringUntil('\n');
    processSerialCommand(line);
  }
}
