class Hello {
    // A friendly greeting
    private val name: String = "world"
    private val initial: Char = 'w'
    private val loud: Boolean = true

    companion object Defaults {
        val fallback: String = "friend"
    }

    fun greet(): String {
        println(name)
        return "Hello, $name!"
    }
}
