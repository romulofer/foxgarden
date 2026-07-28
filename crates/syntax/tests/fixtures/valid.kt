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

enum class Level {
    LOW, MEDIUM, HIGH
}

inline fun <reified T> isInstance(value: Any): Boolean = value is T

suspend fun fetchData(): String = "data"

val doubled: (Int) -> Int = { it * 2 }

var counter: Int = 0
    get() = field
    set(value) {
        field = value
    }
