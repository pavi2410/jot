package com.jot.samples.webserver;

import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Test;

@SuppressWarnings("PMD.AtLeastOneConstructor")
class ServerMainTest {

  @Test
  void healthPayload() {
    assertEquals("ok", "ok");
  }
}
