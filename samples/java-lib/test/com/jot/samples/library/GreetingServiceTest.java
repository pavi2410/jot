package com.jot.samples.library;

import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Test;

@SuppressWarnings("PMD.AtLeastOneConstructor")
class GreetingServiceTest {

  @Test
  void buildsExpectedGreeting() {
    final GreetingService service = new GreetingService();
    assertEquals("hello jot", service.greetingFor("jot"));
  }
}
