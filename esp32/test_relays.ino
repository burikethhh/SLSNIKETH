#include <Arduino.h>

/*
  SOLENOID LOCK TESTER
  --------------------
  GPIO18 = Solenoid relay (active-LOW: HIGH=locked, LOW=unlocked)
  GPIO25 = Piezo buzzer
  
  Default: LOCKED (GPIO18 HIGH = relay OFF = solenoid OUT)
  
  Commands via Serial (115200):
    LOCK    = lock door (solenoid OUT)
    UNLOCK  = unlock door (solenoid IN)
    TEST    = full cycle: lock → beep → unlock 5s → beep → lock
    STATUS  = show pin states
*/

#define SOLENOID_PIN 18
#define BUZZER_PIN   25

void beep(int count, int ms) {
  for (int i = 0; i < count; i++) {
    digitalWrite(BUZZER_PIN, HIGH);
    delay(ms);
    digitalWrite(BUZZER_PIN, LOW);
    if (i < count - 1) delay(ms);
  }
}

void lockDoor() {
  digitalWrite(SOLENOID_PIN, HIGH);  // relay OFF = solenoid OUT = locked
}

void unlockDoor() {
  digitalWrite(SOLENOID_PIN, LOW);   // relay ON = solenoid IN = unlocked
}

void printStatus() {
  int state = digitalRead(SOLENOID_PIN);
  Serial.print(">> GPIO18=");
  Serial.print(state);
  Serial.print(" → ");
  Serial.println(state == HIGH ? "LOCKED (relay OFF, solenoid OUT)" : "UNLOCKED (relay ON, solenoid IN)");
}

void setup() {
  // Set relay pin HIGH FIRST (lock by default)
  pinMode(SOLENOID_PIN, OUTPUT);
  digitalWrite(SOLENOID_PIN, HIGH);
  
  pinMode(BUZZER_PIN, OUTPUT);
  digitalWrite(BUZZER_PIN, LOW);

  Serial.begin(115200);
  delay(500);

  Serial.println();
  Serial.println("=== SOLENOID LOCK TESTER ===");
  Serial.println("Default: LOCKED");
  printStatus();
  Serial.println();
  Serial.println("Commands: LOCK, UNLOCK, TEST, STATUS");
  
  // Boot beep
  beep(2, 100);
}

void loop() {
  if (!Serial.available()) return;
  
  String cmd = Serial.readStringUntil('\n');
  cmd.trim();
  cmd.toUpperCase();

  if (cmd == "LOCK") {
    lockDoor();
    beep(1, 200);
    Serial.println(">> LOCKED");
    printStatus();
  }
  else if (cmd == "UNLOCK") {
    unlockDoor();
    beep(2, 100);
    Serial.println(">> UNLOCKED");
    printStatus();
  }
  else if (cmd == "TEST") {
    Serial.println(">> === SOLENOID CYCLE TEST ===");
    Serial.println(">> 5s OFF (locked) / 5s ON (unlocked) — repeating 5 times");
    Serial.println(">> Send any command to stop");
    
    for (int i = 0; i < 5; i++) {
      // LOCKED for 5s
      lockDoor();
      beep(1, 200);
      Serial.print(">> [");
      Serial.print(i + 1);
      Serial.println("/5] LOCKED (solenoid OUT) — 5 seconds...");
      printStatus();
      delay(5000);
      
      // Check if user wants to stop
      if (Serial.available()) {
        Serial.readStringUntil('\n');
        Serial.println(">> Stopped by user");
        break;
      }
      
      // UNLOCKED for 5s
      unlockDoor();
      beep(2, 100);
      Serial.print(">> [");
      Serial.print(i + 1);
      Serial.println("/5] UNLOCKED (solenoid IN) — 5 seconds...");
      printStatus();
      delay(5000);
      
      // Check if user wants to stop
      if (Serial.available()) {
        Serial.readStringUntil('\n');
        Serial.println(">> Stopped by user");
        break;
      }
    }
    
    // End locked
    lockDoor();
    beep(1, 300);
    Serial.println(">> === TEST COMPLETE — LOCKED ===");
    printStatus();
  }
  else if (cmd == "STATUS") {
    printStatus();
  }
  else {
    Serial.println("Unknown: " + cmd);
    Serial.println("Commands: LOCK, UNLOCK, TEST, STATUS");
  }
}
