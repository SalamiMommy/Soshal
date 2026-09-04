// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/vouch_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-vouch');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('VouchService', () {
    test('publish returns event id and passes target/content through',
        () async {
      final vouch = VouchService();
      api.stub('crateFfiVouchVouchPublish', (_) async => 'ev-vouch-1');

      final id = await vouch.publish('pk-target', 'trust this dev');
      expect(id, 'ev-vouch-1');
      expect(vouch.lastError, isNull);

      final inv = api.callsOf('crateFfiVouchVouchPublish').single;
      expect(api.namedArg(inv, 'targetPubkey'), 'pk-target');
      expect(api.namedArg(inv, 'content'), 'trust this dev');
    });

    test('publish failure sets lastError and rethrows', () async {
      final vouch = VouchService();
      api.stub('crateFfiVouchVouchPublish',
          (_) => throw Exception('sign failed'));

      await expectLater(
          vouch.publish('pk-target', 'x'), throwsException);
      expect(vouch.lastError, contains('sign failed'));
    });

    test('fetch parses verified vouches, updates list and notifies',
        () async {
      final vouch = VouchService();
      var notified = 0;
      vouch.addListener(() => notified++);
      api.stub(
        'crateFfiVouchVouchFetch',
        (_) async =>
            '[{"id":"v-1","pubkey":"pk-voucher","content":"trust this dev",'
            '"created_at":1700000003}]',
      );

      final entries = await vouch.fetch('pk-target');
      expect(entries.single.id, 'v-1');
      expect(entries.single.pubkey, 'pk-voucher');
      expect(entries.single.content, 'trust this dev');
      expect(entries.single.createdAt, 1700000003);
      expect(vouch.vouches.single.content, 'trust this dev');
      expect(vouch.lastError, isNull);
      expect(notified, 1);

      final inv = api.callsOf('crateFfiVouchVouchFetch').single;
      expect(api.namedArg(inv, 'targetPubkey'), 'pk-target');
    });

    test('fetch with empty list clears vouches', () async {
      final vouch = VouchService();
      api.stub('crateFfiVouchVouchFetch',
          (_) async => '[{"id":"v-1","pubkey":"pk","content":"c","created_at":1}]');
      await vouch.fetch('pk-target');
      expect(vouch.vouches, isNotEmpty);

      api.stub('crateFfiVouchVouchFetch', (_) async => '[]');
      final entries = await vouch.fetch('pk-target');
      expect(entries, isEmpty);
      expect(vouch.vouches, isEmpty);
    });

    test('fetch failure sets lastError and rethrows', () async {
      final vouch = VouchService();
      api.stub('crateFfiVouchVouchFetch',
          (_) => throw Exception('verify failed'));

      await expectLater(vouch.fetch('pk-target'), throwsException);
      expect(vouch.lastError, contains('verify failed'));
    });
  });
}
