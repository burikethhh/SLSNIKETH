/*
  GymPOS Access Controller — ESP32 Firmware (Arduino IDE Single-File)
  ──────────────────────────────────────────────────────────────────
  Receives commands via USB Serial from the GymPOS Desktop application.
  Controls door solenoid/relay, buzzer, and 16x2 I2C LCD.
  RFID is handled by external USB reader on the PC side.

  Pin Wiring (Field Connections):
    GPIO18  → LCD SDA           (software I2C, Wire.begin(18, 19))
    GPIO19  → LCD SCL           (software I2C)
    GPIO9   → 5V Buzzer         (active HIGH)
    5V      → LCD VCC & Buzzer VCC
    GND     → Common Ground

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
    - By default, LOCK_PIN is -1 for direct-wired relay power paths.
    - If a GPIO pin is defined (LOCK_PIN >= 0):
        relay OFF = pinMode(INPUT)  → high impedance → locked
        relay ON  = OUTPUT + LOW    → pulls IN to GND → unlocked

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
    ALERT_TAILGATE         → rapid pulsing alarm (~9s) + LCD warning
    ALARM                  → alias for ALERT_TAILGATE
    LCD:<line1>|<line2>    → display custom text on LCD (| separates lines)
    LCD_CLEAR              → clear LCD and return to idle screen
    PING                   → reply ACK:PONG (health check)
    STATUS                 → reply with current lock state (ACK:STATUS:LOCKED|UNLOCKED)

  Messages Sent TO PC:
    READY                  → sent once on boot
    ACK:<cmd>              → acknowledgement for every command
    ACK:RELOCK             → auto-lock notification after unlock timer expires
*/

#include <Arduino.h>
#include <Wire.h>
#include <LiquidCrystal_I2C.h>

// ───────────── Lock Configuration ─────────────
#ifndef LOCK_PIN
#define LOCK_PIN -1  // -1 = Direct-wired relay / logical tracking only
#endif

// ───────────── Pin Configuration ─────────────
// Standard 4-pin I2C LCD backpack: SDA, SCL, GND, 5V
#ifndef LCD_SDA_PIN
static const int LCD_SDA_PIN    = 18;  // I2C SDA (Wire.begin(18, 19))
#endif
#ifndef LCD_SCL_PIN
static const int LCD_SCL_PIN    = 19;  // I2C SCL
#endif
#ifndef BUZZER_PIN
static const int BUZZER_PIN     = 10;  // 5V active buzzer on safe GPIO10 (NOT GPIO9 strapping pin)
#endif

// ───────────── I2C Pin & Address Auto-Detection ─────────────
struct I2cResult {
  int sda;
  int scl;
  uint8_t addr;
};

int activeSdaPin = LCD_SDA_PIN;
int activeSclPin = LCD_SCL_PIN;

static I2cResult scanAllPinsForLcd() {
  Serial.println("Auto-scanning board pins for I2C LCD backpack...");
  const uint8_t candidateAddrs[] = {0x27, 0x3F, 0x26, 0x38, 0x20};

  // Priority pin pairs (18/19, reversed, common C3 alternate pins)
  const int priorityPairs[][2] = {
    {18, 19}, {19, 18},
    {6, 7},   {7, 6},
    {4, 5},   {5, 4},
    {8, 9},   {9, 8},
    {0, 1},   {1, 0},
    {2, 3},   {3, 2},
    {10, 18}, {18, 10}
  };

  for (auto& pair : priorityPairs) {
    int sda = pair[0];
    int scl = pair[1];
    Wire.end();
    Wire.setPins(sda, scl);
    Wire.begin(sda, scl);
    Wire.setTimeOut(30);
    Wire.setClock(100000);
    for (uint8_t a : candidateAddrs) {
      Wire.beginTransmission(a);
      if (Wire.endTransmission() == 0) {
        Serial.printf("  >>> SUCCESS: I2C LCD found on SDA=GPIO%d, SCL=GPIO%d, Addr=0x%02X <<<\n", sda, scl, a);
        return {sda, scl, a};
      }
    }
  }

  // Full sweep across all GPIO header pins
  const int allHeaderPins[] = {18, 19, 6, 7, 4, 5, 8, 9, 0, 1, 2, 3, 10};
  const int count = sizeof(allHeaderPins) / sizeof(allHeaderPins[0]);
  for (int i = 0; i < count; i++) {
    for (int j = 0; j < count; j++) {
      if (i == j) continue;
      int sda = allHeaderPins[i];
      int scl = allHeaderPins[j];
      Wire.end();
      Wire.setPins(sda, scl);
      Wire.begin(sda, scl);
      Wire.setTimeOut(15);
      Wire.setClock(100000);
      for (uint8_t a : candidateAddrs) {
        Wire.beginTransmission(a);
        if (Wire.endTransmission() == 0) {
          Serial.printf("  >>> SUCCESS: I2C LCD found on SDA=GPIO%d, SCL=GPIO%d, Addr=0x%02X <<<\n", sda, scl, a);
          return {sda, scl, a};
        }
      }
    }
  }

  Serial.println("  No I2C device ACKed on any pin pair. Defaulting to SDA=18, SCL=19, Addr=0x27");
  return {18, 19, 0x27};
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
  lcd->print(" GymPOS by SLS");
  lcd->setCursor(0, 1);
  lcd->print("   Scan Face    ");
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
  String trimmed = cmdRaw;
  trimmed.trim();
  if (trimmed.length() == 0) return;

  int colonIdx = trimmed.indexOf(':');
  String base = (colonIdx >= 0) ? trimmed.substring(0, colonIdx) : trimmed;
  base.trim();
  base.toUpperCase();

  String argRaw = (colonIdx >= 0) ? trimmed.substring(colonIdx + 1) : "";
  argRaw.trim();

  // WELCOME
  if (base == "WELCOME") {
    int pipeIdx = argRaw.indexOf('|');
    String name = (pipeIdx >= 0) ? argRaw.substring(0, pipeIdx) : argRaw;
    name.trim();
    unsigned long ms = (pipeIdx >= 0) ? parseSeconds(argRaw.substring(pipeIdx + 1), DEFAULT_UNLOCK_MS) : DEFAULT_UNLOCK_MS;
    grantWelcome(name, ms);
    return;
  }

  // BYE
  if (base == "BYE" || base == "GOODBYE") {
    int pipeIdx = argRaw.indexOf('|');
    String name = (pipeIdx >= 0) ? argRaw.substring(0, pipeIdx) : argRaw;
    name.trim();
    unsigned long ms = (pipeIdx >= 0) ? parseSeconds(argRaw.substring(pipeIdx + 1), DEFAULT_UNLOCK_MS) : DEFAULT_UNLOCK_MS;
    grantBye(name, ms);
    return;
  }

  // UNLOCK
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
    int pipe = argRaw.indexOf('|');
    String l1 = (pipe >= 0) ? argRaw.substring(0, pipe) : argRaw;
    String l2 = (pipe >= 0) ? argRaw.substring(pipe + 1) : "";
    lcdShow(l1, l2, 8000);
    Serial.println("ACK:LCD");
    return;
  }

  // LCD_CLEAR
  if (base == "LCD_CLEAR") {
    lcdShowIdle();
    Serial.println("ACK:LCD_CLEAR");
    return;
  }

  // LCD_REINIT
  if (base == "LCD_REINIT") {
    I2cResult res = scanAllPinsForLcd();
    initLcdHardware(res.sda, res.scl, res.addr);
    Serial.printf("ACK:LCD_REINIT:SDA=%d:SCL=%d:ADDR=0x%02X\n", res.sda, res.scl, res.addr);
    return;
  }

  // PING
  if (base == "PING") {
    Serial.println("ACK:PONG");
    return;
  }

  // STATUS
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
  pinMode(BUZZER_PIN, OUTPUT);
  digitalWrite(BUZZER_PIN, LOW);
  setLocked(true);

  Serial.begin(115200);
  delay(100);
  Serial.setTimeout(50);
  Serial.println("\nGymPOS Controller Booting...");

  // Immediate audible feedback that MCU has booted
  buzzerStart(PAT_STARTUP);

  // Automatically probe all board pins for the I2C LCD backpack
  I2cResult res = scanAllPinsForLcd();
  initLcdHardware(res.sda, res.scl, res.addr);

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
