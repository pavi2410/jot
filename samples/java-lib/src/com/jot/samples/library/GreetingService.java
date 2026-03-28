package com.jot.samples.library;

@SuppressWarnings("PMD.AtLeastOneConstructor")
public final class GreetingService {

  public String greetingFor(final String value) {
    return "hello " + value;
  }
}
