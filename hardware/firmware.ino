/*
  GymPOS Access Controller â€” ESP32 Firmware
  â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
  Receives commands via USB Serial from the GymPOS PC application.
  Controls door solenoid, buzzer, and 16x2 I2C LCD.
  RFID is handled by external USB reader on the PC side.

  Pin Wiring:
    GPIO18  â†’ Solenoid Relay   (5V relay, INPUT mode trick)
                VCCâ†’5V, GNDâ†’GND, INâ†’GPIO18
                12V+ â†’ Relay COM, Relay NO â†’ Solenoid+
    GPIO21  â†’ LCD SDA           (I2C)
    GPIO22  â†’ LCD SCL           (I2C)
    GPIO25  â†’ 5V Buzzer         (active HIGH)
    GPIO2   â†’ Built-in LED      (status indicator)

  Relay Control (5V relay + 3.3V GPIO workaround):
    relay OFF = pinMode(INPUT)  â†’ high impedance â†’ solenoid OUT (locked)
    relay ON  = OUTPUT + LOW    â†’ pulls IN to GND â†’ solenoid IN (unlocked)

  Serial commands (case-insensitive, \n terminated):
    UNLOCK             â†’ open solenoid 5s + success beep + LCD "Access Granted"
    UNLOCK:<seconds>   â†’ open solenoid for custom duration
    LOCK               â†’ force-lock immediately
    DENY               â†’ triple beep + LCD "Access Denied"
    DENY:<reason>      â†’ triple beep + LCD shows reason text
    BEEP               â†’ single short beep
    BEEP_LONG          â†’ long 2-second beep (tailgate alarm)
    LCD:<line1>|<line2> â†’ display custom text on LCD (| separates lines)
    LCD_CLEAR          â†’ clear LCD and show idle screen
    PING               â†’ reply ACK:PONG (health check)
    STATUS             â†’ reply with current lock state

  Messages sent TO the PC:
    READY              â†’ sent once on boot
    ACK:<cmd>          â†’ acknowledgement for every command
    ACK:RELOCK         â†’ auto-lock after unlock timer expires
*/

#include <Arduino.h>
#include <Wire.h>
#include <LiquidCrystal_I2C.h>

// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€ Pin Configuration â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
static const int SOLENOID_PIN   = 18;  // Solenoid relay (5V relay, INPUT mode trick)
static const int LCD_SDA_PIN    = 21;  // I2C SDA (Wire default)
static const int LCD_SCL_PIN    = 22;  // I2C SCL (Wire default)
static const int BUZZER_PIN     = 25;  // 5V active buzzer
static const int STATUS_LED_PIN = 2;   // Built-in LED

// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€ Peripherals â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
LiquidCrystal_I2C lcd(0x27, 16, 2);   // Common I2C address; change to 0x3F if needed

// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€ Timing State â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
static const unsigned long DEFAULT_UNLOCK_MS = 5000;

unsigned long unlockUntil   = 0;
unsigned long lcdIdleAfter  = 0;       // auto-return to idle screen
bool isLocked               = true;

// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€ Custom LCD Characters â”€â”€â”€â”€â”€â”€â”€â”€â”€
// Lock icon
byte lockChar[8] = {0b01110,0b10001,0b10001,0b11111,0b11011,0b11011,0b11111,0b00000};
// Unlock icon
byte unlockChar[8] = {0b01110,0b10000,0b10000,0b11111,0b11011,0b11011,0b11111,0b00000};
// Alert icon
byte alertChar[8] = {0b00100,0b00100,0b01110,0b01110,0b11111,0b11111,0b00100,0b00000};

// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€ Non-blocking Buzzer â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// Plays patterns without delay() so the main loop keeps running.
// Pattern = array of durations: positive = buzzer ON, negative = silence.
static const int PAT_SUCCESS[]  = {120, -100, 120, 0};          // 2 equal short beeps (face scan granted)
static const int PAT_DENY[]     = {100, -80, 100, -80, 100, 0}; // triple beep
static const int PAT_EXIT[]     = {80, -60, 80, 0};             // two short beeps
static const int PAT_STARTUP[]  = {60, -40, 80, -40, 100, 0};  // ascending chirp
static const int PAT_BEEP[]       = {150, 0};                     // single beep
static const int PAT_BEEP_LONG[]  = {2000, 0};                     // long 2s alarm beep
// Tailgate heavy alert: 30 rapid pulses (~9 s total)
static const int PAT_HEAVY_ALERT[]= {
  200,-100, 200,-100, 200,-100, 200,-100, 200,-100,
  200,-100, 200,-100, 200,-100, 200,-100, 200,-100,
  200,-100, 200,-100, 200,-100, 200,-100, 200,-100,
  200,-100, 200,-100, 200,-100, 200,-100, 200,-100,
  200,-100, 200,-100, 200,-100, 200,-100, 200,-100,
  200,-100, 200,-100, 200,-100, 200,-100, 200,-100,
  0
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

// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€ LCD Helpers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
void lcdShowIdle() {
  lcd.clear();
  lcd.setCursor(0, 0);
  lcd.write(0);  // lock icon
  lcd.print(" GYMPOS READY");
  lcd.setCursor(0, 1);
  lcd.print("Scan face/RFID");
}

void lcdShow(const String& line1, const String& line2, unsigned long autoIdleMs = 5000) {
  lcd.clear();
  lcd.setCursor(0, 0);
  lcd.print(line1.substring(0, 16));
  if (line2.length() > 0) {
    lcd.setCursor(0, 1);
    lcd.print(line2.substring(0, 16));
  }
  if (autoIdleMs > 0) {
    lcdIdleAfter = millis() + autoIdleMs;
  } else {
    lcdIdleAfter = 0;
  }
}

// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€ Lock / Unlock â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// INPUT mode trick: 5V relay can't be turned off by 3.3V HIGH (leaks current).
// Instead: INPUT = high impedance (like disconnecting wire) = relay OFF = locked
//          OUTPUT LOW = pulls relay IN to GND = relay ON = unlocked
void setLocked(bool locked) {
  isLocked = locked;
  if (locked) {
    pinMode(SOLENOID_PIN, INPUT);     // relay OFF â†’ solenoid OUT (locked)
  } else {
    pinMode(SOLENOID_PIN, OUTPUT);
    digitalWrite(SOLENOID_PIN, LOW);  // relay ON â†’ solenoid IN (unlocked)
  }
  digitalWrite(STATUS_LED_PIN, locked ? LOW : HIGH);
}

void unlockDoor(unsigned long ms) {
  unlockUntil = millis() + ms;
  setLocked(false);
  buzzerStart(PAT_SUCCESS);
  lcd.clear();
  lcd.setCursor(0, 0);
  lcd.write(1);  // unlock icon
  lcd.print(" ACCESS GRANTED");
  lcd.setCursor(0, 1);
  lcd.print("Door open ");
  lcd.print(ms / 1000);
  lcd.print("s");
  lcdIdleAfter = millis() + ms + 1000;
  Serial.println("ACK:UNLOCK");
}

// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€ Helpers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
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
  // Parse "UNLOCK:10" â†’ 10000ms, fallback to defaultMs
  if (arg.length() == 0) return defaultMs;
  long secs = arg.toInt();
  if (secs <= 0) return defaultMs;
  if (secs > 300) secs = 300;  // cap at 5 minutes
  return (unsigned long)secs * 1000UL;
}

// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€ Command Parser â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
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

  // â”€â”€ UNLOCK â”€â”€
  if (base == "UNLOCK") {
    unsigned long ms = parseSeconds(arg, DEFAULT_UNLOCK_MS);
    unlockDoor(ms);
    return;
  }

  // â”€â”€ LOCK â”€â”€
  if (base == "LOCK") {
    unlockUntil = 0;
    setLocked(true);
    lcdShow("\x00 DOOR LOCKED", "");
    Serial.println("ACK:LOCK");
    return;
  }

  // â”€â”€ DENY â”€â”€
  if (base == "DENY") {
    buzzerStart(PAT_DENY);
    lcd.clear();
    lcd.setCursor(0, 0);
    lcd.print("  ACCESS DENIED");
    lcd.setCursor(0, 1);
    if (arg.length() > 0) {
      // Show reason from PC: DENY:Expired, DENY:Not a member, etc.
      String reason = cmdRaw.substring(colonIdx + 1);
      reason.trim();
      lcd.print(reason.substring(0, 16));
    } else {
      lcd.print("  Unauthorized");
    }
    lcdIdleAfter = millis() + 3000;
    Serial.println("ACK:DENY");
    return;
  }

  // â”€â”€ BEEP â”€â”€
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

  // ALERT_TAILGATE -- rapid buzzer alarm + LCD warning
  if (cmd == "ALERT_TAILGATE") {
    buzzerStart(PAT_HEAVY_ALERT);
    lcd.clear();
    lcd.setCursor(0, 0);
    lcd.write(byte(2));  // alert icon
    lcd.print(" TAILGATE ALERT");
    lcd.setCursor(0, 1);
    lcd.print("Multiple entries!");
    lcdIdleAfter = millis() + 9000;
    Serial.println("ACK:ALERT_TAILGATE");
    return;
  }

  // â”€â”€ LCD â”€â”€  (LCD:Hello World|Line 2)
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

  // â”€â”€ LCD_CLEAR â”€â”€
  if (cmd == "LCD_CLEAR") {
    lcdShowIdle();
    Serial.println("ACK:LCD_CLEAR");
    return;
  }

  // â”€â”€ PING â”€â”€
  if (cmd == "PING") {
    Serial.println("ACK:PONG");
    return;
  }

  // â”€â”€ STATUS â”€â”€
  if (cmd == "STATUS") {
    Serial.print("ACK:STATUS:");
    Serial.println(isLocked ? "LOCKED" : "UNLOCKED");
    return;
  }

  Serial.print("ACK:UNKNOWN:");
  Serial.println(cmd);
}

// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€ Setup â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
void setup() {
  // Pin modes
  pinMode(BUZZER_PIN, OUTPUT);
  pinMode(STATUS_LED_PIN, OUTPUT);

  // Default states â€” everything off, door locked
  digitalWrite(BUZZER_PIN, LOW);
  setLocked(true);  // uses INPUT mode trick to turn relay OFF

  // Serial
  Serial.begin(115200);
  while (!Serial) { delay(5); }

  // I2C LCD
  Wire.begin(LCD_SDA_PIN, LCD_SCL_PIN);
  lcd.init();
  lcd.backlight();
  lcd.createChar(0, lockChar);
  lcd.createChar(1, unlockChar);
  lcd.createChar(2, alertChar);

  // Serial timeout â€” prevent readStringUntil from blocking >50ms
  Serial.setTimeout(50);

  // Startup feedback
  buzzerStart(PAT_STARTUP);
  lcdShowIdle();

  Serial.println("READY");
}

// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€ Main Loop â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
void loop() {
  unsigned long now = millis();

  // â”€â”€ Non-blocking buzzer tick â”€â”€
  buzzerTick();

  // â”€â”€ Auto-relock after unlock timer â”€â”€
  if (unlockUntil > 0 && (long)(now - unlockUntil) >= 0) {
    unlockUntil = 0;
    setLocked(true);
    Serial.println("ACK:RELOCK");
  }

  // â”€â”€ LCD auto-return to idle â”€â”€
  if (lcdIdleAfter > 0 && (long)(now - lcdIdleAfter) >= 0) {
    lcdIdleAfter = 0;
    lcdShowIdle();
  }

  // â”€â”€ Serial command handling â”€â”€
  if (Serial.available()) {
    String line = Serial.readStringUntil('\n');
    processSerialCommand(line);
  }

}

