/*
  GymPOS Access Controller — ESP32 Firmware
  ──────────────────────────────────────────
  Receives commands via USB Serial from the GymPOS PC application.
  Controls door solenoid, buzzer, and 16x2 I2C LCD.
  RFID is handled by external USB reader on the PC side.

  Pin Wiring:
    Relay   → Direct COM/NC (no ESP pin, maglock power via board relay)
    GPIO21  → LCD SDA           (I2C)
    GPIO22  → LCD SCL           (I2C)
    GPIO25  → 5V Buzzer         (active HIGH)
    GPIO2   → Built-in LED      (status indicator)

  Relay: direct-wired, ESP only signals LED/buzzer/LCD; unlock is
  logical (ACK + beep) — physical release is via board power path.

  Serial commands (case-insensitive, \n terminated):
    UNLOCK             → open solenoid 5s + success beep + LCD "Access Granted"
    UNLOCK:<seconds>   → open solenoid for custom duration
    LOCK               → force-lock immediately
    DENY               → triple beep + LCD "Access Denied"
    DENY:<reason>      → triple beep + LCD shows reason text
    BEEP               → single short beep
    LCD:<line1>|<line2> → display custom text on LCD (| separates lines)
    LCD_CLEAR          → clear LCD and show idle screen
    PING               → reply ACK:PONG (health check)
    STATUS             → reply with current lock state

  Messages sent TO the PC:
    READY              → sent once on boot
    ACK:<cmd>          → acknowledgement for every command
    ACK:RELOCK         → auto-lock after unlock timer expires
*/

#include <Arduino.h>
#include <Wire.h>
#include <LiquidCrystal_I2C.h>

// ───────────── Pin Configuration ─────────────
// Relay is direct COM/NC — no ESP pin (was SOLENOID_PIN=18, removed).
static const int LCD_SDA_PIN    = 21;  // I2C SDA (Wire default)
static const int LCD_SCL_PIN    = 22;  // I2C SCL (Wire default)
static const int BUZZER_PIN     = 25;  // 5V active buzzer
static const int STATUS_LED_PIN = 2;   // Built-in LED

// ───────────── I2C Scanner ───────────────────
static uint8_t scanLcdAddress() {
  Serial.println("Scanning I2C bus for devices...");
  uint8_t found = 0;
  for (uint8_t addr = 1; addr < 127; addr++) {
    Wire.beginTransmission(addr);
    if (Wire.endTransmission() == 0) {
      Serial.print("  I2C device found at 0x");
      Serial.println(addr, HEX);
      // Prefer addresses in common LCD backpack range
      if (!found && ((addr >= 0x20 && addr <= 0x2F) || (addr >= 0x38 && addr <= 0x3F)))
        found = addr;
    }
  }
  if (found) {
    Serial.print("Using LCD at 0x");
    Serial.println(found, HEX);
  } else {
    Serial.println("WARNING: No LCD backpack found on I2C bus!");
  }
  return found;
}

// ───────────── Peripherals ───────────────────
LiquidCrystal_I2C* lcd = nullptr;     // Dynamically created after I2C address detection

// ───────────── Timing State ──────────────────
static const unsigned long DEFAULT_UNLOCK_MS = 5000;

unsigned long unlockUntil   = 0;
unsigned long lcdIdleAfter  = 0;       // auto-return to idle screen
bool isLocked               = true;

// ───────────── Custom LCD Characters ─────────
// Lock icon
byte lockChar[8] = {0b01110,0b10001,0b10001,0b11111,0b11011,0b11011,0b11111,0b00000};
// Unlock icon
byte unlockChar[8] = {0b01110,0b10000,0b10000,0b11111,0b11011,0b11011,0b11111,0b00000};
// Alert icon
byte alertChar[8] = {0b00100,0b00100,0b01110,0b01110,0b11111,0b11111,0b00100,0b00000};

// ───────────── Non-blocking Buzzer ───────────
// Plays patterns without delay() so the main loop keeps running.
// Pattern = array of durations: positive = buzzer ON, negative = silence.
static const int PAT_SUCCESS[]  = {120, -100, 120, 0};          // 2 equal short beeps (face scan granted)
static const int PAT_DENY[]     = {100, -80, 100, -80, 100, 0}; // triple beep (denied)
static const int PAT_EXIT[]     = {80, -60, 80, 0};             // two short beeps (exit)
static const int PAT_STARTUP[]  = {60, -40, 80, -40, 100, 0};  // ascending chirp (boot)
static const int PAT_BEEP[]     = {150, 0};                     // single beep
static const int PAT_HEAVY_ALERT[] = {                           // ~10s rapid pulsing (tailgate alarm)
  200,-100,200,-100,200,-100,200,-100,200,-100,
  200,-100,200,-100,200,-100,200,-100,200,-100,
  200,-100,200,-100,200,-100,200,-100,200,-100,
  200,-100,200,-100,200,-100,200,-100,200,-100,
  200,-100,200,-100,200,-100,200,-100,200,-100,
  200,-100,200,-100,200,-100,200,-100,200,-100, 0
};

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
  if ((long)(millis() - buzzerUntil) < 0) return;  // overflow-safe compare

  // Advance to next step
  buzzerStep++;
  int dur = buzzerPattern[buzzerStep];
  if (dur == 0) {
    // Pattern done
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

// ───────────── LCD Helpers ───────────────────
void lcdShowIdle() {
  lcd->clear();
  lcd->setCursor(0, 0);
  lcd->write(0);  // lock icon
  lcd->print(" GYMPOS READY");
  lcd->setCursor(0, 1);
  lcd->print("Scan face/RFID");
}

void lcdShow(const String& line1, const String& line2, unsigned long autoIdleMs = 5000) {
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
// Relay is direct COM/NC — no ESP pin. Keep logical lock state for
// STATUS/RELOCK ACKs and LED only; physical release is board power path.
void setLocked(bool locked) {
  isLocked = locked;
  digitalWrite(STATUS_LED_PIN, locked ? LOW : HIGH);
}

void unlockDoor(unsigned long ms) {
  unlockUntil = millis() + ms;
  setLocked(false);
  buzzerStart(PAT_SUCCESS);
  lcd->clear();
  lcd->setCursor(0, 0);
  lcd->write(1);  // unlock icon
  lcd->print(" ACCESS GRANTED");
  lcd->setCursor(0, 1);
  lcd->print("Door open ");
  lcd->print(ms / 1000);
  lcd->print("s");
  lcdIdleAfter = millis() + ms + 1000;
  Serial.println("ACK:UNLOCK");
}

// ───────────── Helpers ───────────────────────
String bytesToHexString(byte* buffer, byte bufferSize) {
  String out = "";
  for (byte i = 0; i < bufferSize; i++) {
    if (buffer[i] < 0x10) out += "0";
    out += String(buffer[i], HEX);
  }
  out.toUpperCase();
  return out;
}

unsigned long parseSeconds(const String& arg, unsigned long defaultMs) {
  // Parse "UNLOCK:10" → 10000ms, fallback to defaultMs
  if (arg.length() == 0) return defaultMs;
  long secs = arg.toInt();
  if (secs <= 0) return defaultMs;
  if (secs > 300) secs = 300;  // cap at 5 minutes
  return (unsigned long)secs * 1000UL;
}

// ───────────── Command Parser ────────────────
void processSerialCommand(const String& cmdRaw) {
  String cmd = cmdRaw;
  cmd.trim();
  cmd.toUpperCase();

  // Split on first ':' for parameterized commands
  String base = cmd;
  String arg  = "";
  int colonIdx = cmd.indexOf(':');
  if (colonIdx >= 0) {
    base = cmd.substring(0, colonIdx);
    arg  = cmd.substring(colonIdx + 1);
  }

  // ── UNLOCK ──
  if (base == "UNLOCK") {
    unsigned long ms = parseSeconds(arg, DEFAULT_UNLOCK_MS);
    unlockDoor(ms);
    return;
  }

  // ── LOCK ──
  if (base == "LOCK") {
    unlockUntil = 0;
    setLocked(true);
    lcdShow("\x00 DOOR LOCKED", "");
    Serial.println("ACK:LOCK");
    return;
  }

  // ── DENY ──
  if (base == "DENY") {
    buzzerStart(PAT_DENY);
    lcd->clear();
    lcd->setCursor(0, 0);
    lcd->print("  ACCESS DENIED");
    lcd->setCursor(0, 1);
    if (arg.length() > 0) {
      // Show reason from PC: DENY:Expired, DENY:Not a member, etc.
      String reason = cmdRaw.substring(colonIdx + 1);
      reason.trim();
      lcd->print(reason.substring(0, 16));
    } else {
      lcd->print("  Unauthorized");
    }
    lcdIdleAfter = millis() + 3000;
    Serial.println("ACK:DENY");
    return;
  }

  // ── BEEP ──
  if (base == "BEEP") {
    buzzerStart(PAT_BEEP);
    Serial.println("ACK:BEEP");
    return;
  }

  // ── ALERT_TAILGATE ──  heavy rapid buzz ~5 seconds
  if (base == "ALERT_TAILGATE") {
    buzzerStart(PAT_HEAVY_ALERT);
    lcd->clear();
    lcd->setCursor(0, 0);
    lcd->write(2);  // alert icon
    lcd->print(" TAILGATE ALERT");
    lcd->setCursor(0, 1);
    lcd->print("Multiple entries!");
    lcdIdleAfter = millis() + 6000;
    Serial.println("ACK:ALERT_TAILGATE");
    return;
  }

  // ── LCD ──  (LCD:Hello World|Line 2)
  if (base == "LCD") {
    String raw = cmdRaw.substring(colonIdx + 1);
    raw.trim();
    int pipe = raw.indexOf('|');
    String l1 = (pipe >= 0) ? raw.substring(0, pipe) : raw;
    String l2 = (pipe >= 0) ? raw.substring(pipe + 1) : "";
    lcdShow(l1, l2, 8000);
    Serial.println("ACK:LCD");
    return;
  }

  // ── LCD_CLEAR ──
  if (cmd == "LCD_CLEAR") {
    lcdShowIdle();
    Serial.println("ACK:LCD_CLEAR");
    return;
  }

  // ── PING ──
  if (cmd == "PING") {
    Serial.println("ACK:PONG");
    return;
  }

  // ── STATUS ──
  if (cmd == "STATUS") {
    Serial.print("ACK:STATUS:");
    Serial.println(isLocked ? "LOCKED" : "UNLOCKED");
    return;
  }

  Serial.print("ACK:UNKNOWN:");
  Serial.println(cmd);
}

// ───────────── Setup ─────────────────────────
void setup() {
  // Pin modes
  pinMode(BUZZER_PIN, OUTPUT);
  pinMode(STATUS_LED_PIN, OUTPUT);

  // Default states — everything off, door locked
  digitalWrite(BUZZER_PIN, LOW);
  setLocked(true);  // uses INPUT mode trick to turn relay OFF

  // Serial
  Serial.begin(115200);
  while (!Serial) { delay(5); }

  // I2C LCD — slower clock helps with longer wires/noise
  Wire.begin(LCD_SDA_PIN, LCD_SCL_PIN);
  Wire.setClock(50000);

  uint8_t lcdAddr = scanLcdAddress();
  if (!lcdAddr) {
    lcdAddr = 0x27;
    Serial.println("Defaulting to 0x27");
  }
  delay(250);  // extra settling time for LCD power-up
  lcd = new LiquidCrystal_I2C(lcdAddr, 16, 2);
  lcd->begin(16, 2);
  lcd->backlight();
  lcd->createChar(0, lockChar);
  lcd->createChar(1, unlockChar);
  lcd->createChar(2, alertChar);

  // Serial timeout — prevent readStringUntil from blocking >50ms
  Serial.setTimeout(50);

  // Startup feedback
  buzzerStart(PAT_STARTUP);
  lcdShowIdle();

  Serial.println("READY");
}

// ───────────── Main Loop ─────────────────────
void loop() {
  unsigned long now = millis();

  // ── Non-blocking buzzer tick ──
  buzzerTick();

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

  // ── Serial command handling ──
  if (Serial.available()) {
    String line = Serial.readStringUntil('\n');
    processSerialCommand(line);
  }

}
