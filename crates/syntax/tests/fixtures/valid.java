public class Hello {
    // A friendly greeting
    private String name;
    private boolean loud = true;

    @SuppressWarnings("unused")
    public String greet() {
        if (this.loud) {
            System.out.println(name);
        }
        return "Hello, " + name + "!";
    }
}
