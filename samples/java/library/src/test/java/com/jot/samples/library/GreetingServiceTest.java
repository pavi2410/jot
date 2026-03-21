package com.jot.samples.library;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;

class GreetingServiceTest {
    @Test
    void buildsExpectedGreeting() {
        GreetingService service = new GreetingService();
        assertEquals("hello jot", service.greetingFor("jot"));
    }
}
