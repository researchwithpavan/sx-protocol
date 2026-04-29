import org.sx.SxMessage;

public class Basic {
    public static void main(String[] args) {
        try (SxMessage m = SxMessage.parseText("{name:\"Asha\"}")) {
            System.out.println(m.toText());
        }
    }
}
