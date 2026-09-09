import com.sun.jna.Library;
import com.sun.jna.Native;

public interface MmDlp extends Library {
    MmDlp INSTANCE = (MmDlp) Native.load("mm_dlp", MmDlp.class);

    // JNA binds 64-bit integers to Java's long primitive
    long add(long left, long right);
}