public class Hello {
    // A friendly greeting
    private String name;
    private boolean loud = true;
    private int mask = 0b1010;
    private static final int MAX_LENGTH = 100;

    @SuppressWarnings("unused")
    public String greet() {
        if (this.loud) {
            System.out.println(name);
        }
        return "Hello, " + name + "!";
    }

    /**
     * Formats a value for display.
     */
    private String formatLength(int length) {
        outer:
        for (int i = 0; i < length; i++) {
            if (i == 5) {
                break outer;
            }
        }
        return length + " chars";
    }
}

record Point(int x, int y) {}

@interface Marker {}

enum Suit { Hearts, Diamonds, Clubs, Spades }
