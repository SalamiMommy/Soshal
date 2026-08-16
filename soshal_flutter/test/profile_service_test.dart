// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/profile_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-profile');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('ProfileService', () {
    test('getCustomProfileNodes forwards pubkey and returns JSON', () {
      const json = '{"themeId":"default","nodes":[]}';
      api.stubString('crateFfiDbDbGetCustomProfileNodes', json);

      final service = ProfileService();
      final result = service.getCustomProfileNodes(pubkey: 'pk-1');

      expect(result, json);
      final inv = api.callsOf('crateFfiDbDbGetCustomProfileNodes').single;
      expect(api.namedArg(inv, 'pubkey'), 'pk-1');
    });

    test('getCustomProfileNodes returns empty string when nothing stored',
        () {
      api.stubString('crateFfiDbDbGetCustomProfileNodes', '');

      final service = ProfileService();
      expect(service.getCustomProfileNodes(pubkey: 'pk-1'), '');
    });

    test('getCustomProfileNodes propagates backend error', () {
      api.stub('crateFfiDbDbGetCustomProfileNodes', (_) {
        throw Exception('db error');
      });

      final service = ProfileService();
      expect(
        () => service.getCustomProfileNodes(pubkey: 'pk-1'),
        throwsA(isA<Exception>()),
      );
    });

    test('saveCustomProfile forwards pubkey and JSON, returns true', () {
      api.stubBool('crateFfiDbDbSaveCustomProfile', true);

      final service = ProfileService();
      final result = service.saveCustomProfile(
        pubkey: 'pk-1',
        profileJson: '{"themeId":"dark"}',
      );

      expect(result, isTrue);
      final inv = api.callsOf('crateFfiDbDbSaveCustomProfile').single;
      expect(api.namedArg(inv, 'pubkey'), 'pk-1');
      expect(api.namedArg(inv, 'profileJson'), '{"themeId":"dark"}');
    });

    test('saveCustomProfile with empty JSON still forwards', () {
      api.stubBool('crateFfiDbDbSaveCustomProfile', false);

      final service = ProfileService();
      final result =
          service.saveCustomProfile(pubkey: 'pk-1', profileJson: '');

      expect(result, isFalse);
      final inv = api.callsOf('crateFfiDbDbSaveCustomProfile').single;
      expect(api.namedArg(inv, 'profileJson'), '');
    });

    test('saveCustomProfile propagates backend error', () {
      api.stub('crateFfiDbDbSaveCustomProfile', (_) {
        throw Exception('write failed');
      });

      final service = ProfileService();
      expect(
        () => service.saveCustomProfile(
          pubkey: 'pk-1',
          profileJson: '{}',
        ),
        throwsA(isA<Exception>()),
      );
    });

    test('npubEncode forwards publicKey and returns npub', () {
      api.stubString('crateFfiAuthAuthNpubEncode', 'npub1abc');

      final service = ProfileService();
      final result = service.npubEncode(publicKey: 'aabb');

      expect(result, 'npub1abc');
      final inv = api.callsOf('crateFfiAuthAuthNpubEncode').single;
      expect(api.namedArg(inv, 'publicKey'), 'aabb');
    });

    test('npubEncode propagates backend error', () {
      api.stub('crateFfiAuthAuthNpubEncode', (_) {
        throw Exception('bad pubkey');
      });

      final service = ProfileService();
      expect(
        () => service.npubEncode(publicKey: 'zz'),
        throwsA(isA<Exception>()),
      );
    });
  });

  group('CustomProfile models', () {
    test('makeDefaultNode builds a node for every registered type', () {
      for (final info in nodeTypes) {
        final node = makeDefaultNode(info.type, 3);
        expect(node.type, info.type);
        expect(node.id, startsWith('widget_'));
        expect(node.position.order, 3);
        expect(node.properties, isNotEmpty);
      }
    });

    test('makeDefaultNode falls back to container for unknown type', () {
      final node = makeDefaultNode('hologram', 0);
      expect(node.type, 'container');
      expect(node.properties['title'], 'Widget');
    });

    test('CustomProfile fromJson applies defaults for missing keys', () {
      final profile = CustomProfile.fromJson(const {});
      expect(profile.themeId, 'default');
      expect(profile.nodes, isEmpty);
    });

    test('CustomProfile toJson round-trips nodes', () {
      final node = makeDefaultNode('text_block', 0);
      final profile = CustomProfile(themeId: 'dark', nodes: [node]);

      final restored = CustomProfile.fromJson(profile.toJson());

      expect(restored.themeId, 'dark');
      expect(restored.nodes.single.id, node.id);
      expect(restored.nodes.single.type, 'text_block');
      final props =
          TextBlockProperties.fromJson(restored.nodes.single.properties);
      expect(props.content, '');
      expect(props.markdownEnabled, isFalse);
    });

    test('CustomProfileNode fromJson tolerates missing styles and position',
        () {
      final node = CustomProfileNode.fromJson(
        const {'id': 'n1', 'type': 'theme'},
      );
      expect(node.styles.toJson(), isNotEmpty);
      expect(node.position.row, 0);
      expect(node.properties, isEmpty);
    });

    test('SanitizedStyles fromJson coerces numbers and copyWith overrides',
        () {
      final styles = SanitizedStyles.fromJson(const {
        'padding': 12.5,
        'fontSize': '14',
        'height': 100,
      });
      expect(styles.padding, 12.5);
      expect(styles.fontSize, '14');
      expect(styles.height, 100);

      final copied = styles.copyWith(padding: 4, offsetY: -2);
      expect(copied.padding, 4);
      expect(copied.offsetY, -2);
      expect(copied.fontSize, '14');
    });
  });
}