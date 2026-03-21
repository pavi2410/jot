package com.jot.samples.workspace.domain;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;

class OrderTest {
    @Test
    void exposesOrderData() {
        Order order = new Order("A-1", "jot");
        assertEquals("A-1", order.id());
    }
}
