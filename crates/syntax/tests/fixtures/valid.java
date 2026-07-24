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
}

record Point(int x, int y) {}

@interface Marker {}
