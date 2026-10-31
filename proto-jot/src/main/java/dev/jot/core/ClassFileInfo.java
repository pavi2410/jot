package dev.jot.core;

import java.io.DataInputStream;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;

/** Reads the class name and source file attribute needed to track javac outputs. */
record ClassFileInfo(String internalName, String sourceFile) {
    static ClassFileInfo read(Path file) throws IOException {
        try (DataInputStream input = new DataInputStream(Files.newInputStream(file))) {
            if (input.readInt() != 0xCAFEBABE) throw new IOException("invalid class file: " + file);
            input.readUnsignedShort(); // minor version
            input.readUnsignedShort(); // major version
            int count = input.readUnsignedShort();
            String[] utf8 = new String[count];
            int[] classNames = new int[count];
            for (int index = 1; index < count; index++) {
                int tag = input.readUnsignedByte();
                switch (tag) {
                    case 1 -> utf8[index] = input.readUTF();
                    case 3, 4 -> skipFully(input, 4);
                    case 5, 6 -> {
                        skipFully(input, 8);
                        index++;
                    }
                    case 7 -> classNames[index] = input.readUnsignedShort();
                    case 8, 16, 19, 20 -> skipFully(input, 2);
                    case 9, 10, 11, 12, 17, 18 -> skipFully(input, 4);
                    case 15 -> skipFully(input, 3);
                    default -> throw new IOException("unknown constant-pool tag " + tag
                            + " in " + file);
                }
            }

            input.readUnsignedShort(); // access flags
            int classIndex = input.readUnsignedShort();
            input.readUnsignedShort(); // superclass
            String internalName = utf8[classNames[classIndex]];
            int interfaceCount = input.readUnsignedShort();
            skipFully(input, interfaceCount * 2L);
            skipMembers(input);
            skipMembers(input);
            String sourceFile = null;
            int attributeCount = input.readUnsignedShort();
            for (int index = 0; index < attributeCount; index++) {
                String attribute = utf8[input.readUnsignedShort()];
                long length = Integer.toUnsignedLong(input.readInt());
                if ("SourceFile".equals(attribute) && length == 2) {
                    sourceFile = utf8[input.readUnsignedShort()];
                } else {
                    skipFully(input, length);
                }
            }
            if (internalName == null) throw new IOException("class name missing in " + file);
            return new ClassFileInfo(internalName, sourceFile);
        }
    }

    private static void skipMembers(DataInputStream input) throws IOException {
        int count = input.readUnsignedShort();
        for (int member = 0; member < count; member++) {
            skipFully(input, 6); // access flags, name, descriptor
            int attributeCount = input.readUnsignedShort();
            for (int attribute = 0; attribute < attributeCount; attribute++) {
                input.readUnsignedShort(); // attribute name index
                skipFully(input, Integer.toUnsignedLong(input.readInt()));
            }
        }
    }

    private static void skipFully(DataInputStream input, long length) throws IOException {
        long remaining = length;
        while (remaining > 0) {
            int skipped = input.skipBytes((int) Math.min(remaining, Integer.MAX_VALUE));
            if (skipped == 0) {
                if (input.read() < 0) throw new IOException("truncated class file");
                skipped = 1;
            }
            remaining -= skipped;
        }
    }
}
