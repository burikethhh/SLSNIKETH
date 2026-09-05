/*
  GymPOS Access Controller — ESP32 Firmware (Arduino IDE Single-File)
  ──────────────────────────────────────────────────────────────────
  Receives commands via USB Serial from the GymPOS Desktop application.
  Controls door solenoid/relay, buzzer, and 16x2 I2C LCD.
  RFID is handled by external USB reader on the PC side.

  Pin Wiring (Relay direct COM/NC — no ESP relay pin):
    GPIO18  → LCD SDA           (software I2C, Wire.begin(18, 19))
    GPIO19  → LCD SCL           (software I2C)
    GPIO9   → 5V Buzzer         (active HIGH)
    GPIO2   → Built-in LED      (status indicator)

  Buttonless Architecture:
    - No physical push-buttons required.
    - Continuous camera feeds with automated face detection & anti-tailgate
      verification handled upstream by the GymPOS PC host application.
    - Tailgate alarm arms automatically on every verified entry/exit.

  Relay Control:
    - By default, LOCK_PIN is -1 for direct-wired relay power paths.
    - If a GPIO pin is defined (LOCK_PIN >= 0):
        relay OFF = pinMode(INPUT)  → high impedance → locked
        relay ON  = OUTPUT + LOW    → pulls IN to GND → unlocked

  Serial Commands (case-insensitive, \n terminated):
    UNLOCK             → open solenoid 5s + success beep + LCD "Access Granted"
    UNLOCK:<seconds>   → open solenoid for custom duration (in seconds or ms)
    LOCK               → force-lock immediately
    DENY               → triple beep + LCD "Access Denied"
    DENY:<reason>      → triple beep + LCD shows reason text
    BEEP               → single short beep
    BEEP_LONG          → long 2-second warning beep
    ALERT_TAILGATE     → rapid pulsing alarm (~9s) + LCD warning
    ALARM              → alias for ALERT_TAILGATE
    LCD:<line1>|<line2> → display custom text on LCD (| separates lines)
    LCD_CLEAR          → clear LCD and return to idle screen
    PING               → reply ACK:PONG (health check)
    STATUS             → reply with current lock state (ACK:STATUS:LOCKED|UNLOCKED)

  Messages Sent TO PC:
    READY              → sent once on boot
    ACK:<cmd>          → acknowledgement for every command
    ACK:RELOCK         → auto-lock notification after unlock timer expires
*/

#include <Arduino.h>
#include <Wire.h>
#include <LiquidCrystal_I2C.h>

// ───────────── Lock Configuration ─────────────
#ifndef LOCK_PIN
#define LOCK_PIN -1  // -1 = Direct-wired relay / logical tracking only
#endif

// ───────────── Pin Configuration ─────────────
#ifndef LCD_SDA_PIN
static const int LCD_SDA_PIN    = 18;  // I2C SDA (Wire.begin(18, 19))
#endif
#ifndef LCD_SCL_PIN
static const int LCD_SCL_PIN    = 19;  // I2C SCL
#endif
#ifndef BUZZER_PIN
static const int BUZZER_PIN     = 9;   // 5V active buzzer
#endif
#ifndef STATUS_LED_PIN
static const int STATUS_LED_PIN = 2;   // Built-in LED indicator
#endif

// ───────────── I2C Scanner ───────────────────
static uint8_t scanLcdAddress() {
  Serial.println("Scanning I2C bus for devices...");
  uint8_t found = 0;
  for (uint8_t addr = 1; addr < 127; addr++) {
    Wire.beginTransmission(addr);
    if (Wire.endTransmission() == 0) {
      Serial.print("  I2C device found at 0x");
      Serial.println(addr, HEX);
      if (!found && ((addr >= 0x20 && addr <= 0x2F) || (addr >= 0x38 && addr <= 0x3F))) {
        found = addr;
      }
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
LiquidCrystal_I2C* lcd = nullptr;

// ───────────── Timing State ──────────────────
static const unsigned long DEFAULT_UNLOCK_MS = 5000;

unsigned long unlockUntil   = 0;
unsigned long lcdIdleAfter  = 0;
bool isLocked               = true;

// ───────────── Custom LCD Characters ─────────
byte lockChar[8]   = {0b01110,0b10001,0b10001,0b11111,0b11011,0b11011,0b11111,0b00000};
byte unlockChar[8] = {0b01110,0b10000,0b10000,0b11111,0b11011,0b11011,0b11111,0b00000};
byte alertChar[8]  = {0b00100,0b00100,0b01110,0b01110,0b11111,0b11111,0b00100,0b00000};

// ───────────── Non-blocking Buzzer ───────────
static const int PAT_SUCCESS[]     = {120, -100, 120, 0};
static const int PAT_DENY[]        = {100, -80, 100, -80, 100, 0};
static const int PAT_EXIT[]        = {80, -60, 80, 0};
static const int PAT_STARTUP[]     = {60, -40, 80, -40, 100, 0};
static const int PAT_BEEP[]        = {150, 0};
static const int PAT_BEEP_LONG[]   = {2000, 0};
static const int PAT_HEAVY_ALERT[] = {
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
  if ((long)(millis() - buzzerUntil) < 0) return;

  buzzerStep++;
  int dur = buzzerPattern[buzzerStep];
  if (dur == 0) {
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
  if (!lcd) return;
  lcd->clear();
  lcd->setCursor(0, 0);
  lcd->write(0);
  lcd->print(" GYMPOS READY");
  lcd->setCursor(0, 1);
  lcd->print("Scan face");
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
    pinMode(LOCK_PIN, INPUT);
  } else {
    pinMode(LOCK_PIN, OUTPUT);
    digitalWrite(LOCK_PIN, LOW);
  }
#endif
  digitalWrite(STATUS_LED_PIN, locked ? LOW : HIGH);
}

void unlockDoor(unsigned long ms) {
  unlockUntil = millis() + ms;
  setLocked(false);
  buzzerStart(PAT_SUCCESS);
  if (lcd) {
    lcd->clear();
    lcd->setCursor(0, 0);
    lcd->write(1);
    lcd->print(" ACCESS GRANTED");
    lcd->setCursor(0, 1);
    lcd->print("Door open ");
    lcd->print(ms / 1000);
    lcd->print("s");
  }
  lcdIdleAfter = millis() + ms + 1000;
  Serial.println("ACK:UNLOCK");
}

// ───────────── Helpers ───────────────────────
unsigned long parseSeconds(const String& arg, unsigned long defaultMs) {
  if (arg.length() == 0) return defaultMs;
  long val = arg.toInt();
  if (val <= 0) return defaultMs;
  if (val > 300) {
    return (val > 300000) ? 300000UL : (unsigned long)val;
  }
  return (unsigned long)val * 1000UL;
}

// ───────────── Command Parser ────────────────
void processSerialCommand(const String& cmdRaw) {
  String cmd = cmdRaw;
  cmd.trim();
  cmd.toUpperCase();

  if (cmd.length() == 0) return;

  String base = cmd;
  String arg  = "";
  int colonIdx = cmd.indexOf(':');
  if (colonIdx >= 0) {
    base = cmd.substring(0, colonIdx);
    arg  = cmd.substring(colonIdx + 1);
  }

  // UNLOCK
  if (base == "UNLOCK") {
    unsigned long ms = parseSeconds(arg, DEFAULT_UNLOCK_MS);
    unlockDoor(ms);
    return;
  }

  // LOCK
  if (base == "LOCK") {
    unlockUntil = 0;
    setLocked(true);
    lcdShow("\x00 DOOR LOCKED", "", 3000);
    Serial.println("ACK:LOCK");
    return;
  }

  // DENY
  if (base == "DENY") {
    buzzerStart(PAT_DENY);
    if (lcd) {
      lcd->clear();
      lcd->setCursor(0, 0);
      lcd->print("  ACCESS DENIED");
      lcd->setCursor(0, 1);
      if (arg.length() > 0) {
        String reason = cmdRaw.substring(colonIdx + 1);
        reason.trim();
        lcd->print(reason.substring(0, 16));
      } else {
        lcd->print("  Unauthorized");
      }
    }
    lcdIdleAfter = millis() + 3000;
    Serial.println("ACK:DENY");
    return;
  }

  // BEEP / BEEP_LONG
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

  // ALERT_TAILGATE / ALARM
  if (base == "ALERT_TAILGATE" || base == "ALARM") {
    buzzerStart(PAT_HEAVY_ALERT);
    if (lcd) {
      lcd->clear();
      lcd->setCursor(0, 0);
      lcd->write(2);
      lcd->print(" TAILGATE ALERT");
      lcd->setCursor(0, 1);
      lcd->print("Multiple entries!");
    }
    lcdIdleAfter = millis() + 9000;
    Serial.println("ACK:ALERT_TAILGATE");
    return;
  }

  // LCD
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

  // LCD_CLEAR
  if (cmd == "LCD_CLEAR") {
    lcdShowIdle();
    Serial.println("ACK:LCD_CLEAR");
    return;
  }

  // PING
  if (cmd == "PING") {
    Serial.println("ACK:PONG");
    return;
  }

  // STATUS
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
  pinMode(BUZZER_PIN, OUTPUT);
  pinMode(STATUS_LED_PIN, OUTPUT);

  digitalWrite(BUZZER_PIN, LOW);
  setLocked(true);

  Serial.begin(115200);
  while (!Serial) { delay(5); }
  Serial.setTimeout(50);

  Wire.begin(LCD_SDA_PIN, LCD_SCL_PIN);
  Wire.setClock(50000);

  uint8_t lcdAddr = scanLcdAddress();
  if (!lcdAddr) {
    lcdAddr = 0x27;
    Serial.println("Defaulting to LCD address 0x27");
  }
  delay(250);

  lcd = new LiquidCrystal_I2C(lcdAddr, 16, 2);
  lcd->begin(16, 2);
  lcd->backlight();
  lcd->createChar(0, lockChar);
  lcd->createChar(1, unlockChar);
  lcd->createChar(2, alertChar);

  buzzerStart(PAT_STARTUP);
  lcdShowIdle();

  Serial.println("READY");
}

// ───────────── Main Loop ─────────────────────
void loop() {
  unsigned long now = millis();

  buzzerTick();

  if (unlockUntil > 0 && (long)(now - unlockUntil) >= 0) {
    unlockUntil = 0;
    setLocked(true);
    Serial.println("ACK:RELOCK");
  }

  if (lcdIdleAfter > 0 && (long)(now - lcdIdleAfter) >= 0) {
    lcdIdleAfter = 0;
    lcdShowIdle();
  }

  if (Serial.available()) {
    String line = Serial.readStringUntil('\n');
    processSerialCommand(line);
  }
}
