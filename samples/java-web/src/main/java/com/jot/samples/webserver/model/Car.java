package com.jot.samples.webserver.model;

public class Car {
    private String make;
    private int numberOfSeats;
    private CarType type;

    public enum CarType { SEDAN, SUV, TRUCK }

    public Car(String make, int numberOfSeats, CarType type) {
        this.make = make;
        this.numberOfSeats = numberOfSeats;
        this.type = type;
    }

    public String getMake() { return make; }
    public int getNumberOfSeats() { return numberOfSeats; }
    public CarType getType() { return type; }
}
