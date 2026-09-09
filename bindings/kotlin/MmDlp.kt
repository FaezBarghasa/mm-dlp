import com.sun.jna.Library
import com.sun.jna.Native

interface MmDlp : Library {
    fun add(left: Long, right: Long): Long

    companion object {
        val INSTANCE: MmDlp = Native.load("mm_dlp", MmDlp::class.java)
    }
}