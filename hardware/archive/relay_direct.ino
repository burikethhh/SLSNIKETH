#include <Arduino.h>

/*
  DIRECT RELAY TEST — INPUT mode trick
  -------------------------------------
  Relay VCC → 5V (V  GPIO26 = relay signal
  
  INPUT mode = relay OFF = solenoid OUT (locked)
  OUTPUT LOW = relay ON  = solenoid IN (unlocked)
*/

#define RELAY_PIN 26

void relayOFF() {
  pinMode(RELAY_PIN, INPUT);  // high impedance = like pulling the wire
}

void relayON() {
  pinMode(RELAY_PIN, OUTPUT);
  digitalWrite(RELAY_PIN, LOW);  // pull to ground = relay activates
}

void setup() {
  relayOFF();  // Start LOCKED
  
  Serial.begin(115200);
  delay(500);
  Serial.println("=== RELAY TEST GPIO26 ===");
  Serial.println("Cycling: 5s LOCKED / 5s UNLOCKED");
  Serial.println();
}

void loop() {
  relayOFF();
  delay(5000);
  relayON();
  delay(5000);
  Serial.println("LOCKED (solenoid OUT, unpowered) — still running...");
}
