import org.sx.SxMessage;

public class Basic {
    public static void main(String[] args) {
        try (SxMessage msg = SxMessage.parseText("{name:\"Asha\"}")) {
            System.out.println(msg.toText());
        }
    }
}
