package com.jot.samples.workspace.domain;

import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Test;

@SuppressWarnings("PMD.AtLeastOneConstructor")
class OrderTest {

  @Test
  void exposesOrderData() {
    final Order order = new Order("A-1", "jot");
    assertEquals("A-1", order.orderId());
  }
}
