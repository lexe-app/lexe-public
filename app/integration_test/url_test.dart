import 'package:flutter_test/flutter_test.dart'
    show expect, isFalse, isTrue, test;
import 'package:integration_test/integration_test.dart'
    show IntegrationTestWidgetsFlutterBinding;
import 'package:url_launcher/url_launcher.dart' as url_launcher;

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  Future<bool> canLaunch(String uri) =>
      url_launcher.canLaunchUrl(Uri.parse(uri));

  test("url_launcher.canLaunchUrl", () async {
    expect(await canLaunch("https://lexe.app"), isTrue);

    // Lexe registers itself as a handler for these.
    expect(await canLaunch("bitcoin:bc1qxyz"), isTrue);
    expect(await canLaunch("lightning:lnbc1xyz"), isTrue);
    expect(await canLaunch("lnurlp:lexe.app"), isTrue);

    expect(await canLaunch("lexe-no-such-scheme:x"), isFalse);
  });
}
