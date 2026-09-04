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

  group('CustomProfile schema (Rust-backed)', () {
    test('nodeTypes loads the 12 types from Rust', () {
      api.stubString(
        'crateFfiContentContentCustomProfileNodeTypes',
        '[{"type":"theme","label":"Theme","icon":"🎨"},'
        '{"type":"container","label":"Container","icon":"📦"}]',
      );

      final service = ProfileService();
      expect(service.nodeTypes.length, 2);
      expect(service.nodeTypes.first.type, 'theme');
      expect(service.nodeTypes.first.label, 'Theme');
      expect(
        api.callsOf('crateFfiContentContentCustomProfileNodeTypes'),
        isNotEmpty,
      );
    });

    test('nodeTypes stays empty when Rust errors', () {
      api.stub('crateFfiContentContentCustomProfileNodeTypes', (_) {
        throw Exception('backend down');
      });

      final service = ProfileService();
      expect(service.nodeTypes, isEmpty);
    });

    test('defaultNode forwards type and index to Rust', () {
      const nodeJson =
          '{"id":"widget_1","type":"text_block","styles":{},'
          '"position":{"row":0,"column":0,"order":3},'
          '"properties":{"content":"","title":"About Me"}}';
      api.stubString(
        'crateFfiContentContentCustomProfileDefaultNode',
        nodeJson,
      );

      final service = ProfileService();
      final json = service.defaultNode(type: 'text_block', index: 3);

      expect(json, nodeJson);
      final inv = api
          .callsOf('crateFfiContentContentCustomProfileDefaultNode')
          .single;
      expect(api.namedArg(inv, 'nodeType'), 'text_block');
      expect(api.namedArg(inv, 'index'), BigInt.from(3));
    });

    test('defaultNode propagates backend error for unknown type', () {
      api.stub('crateFfiContentContentCustomProfileDefaultNode', (_) {
        throw Exception('unknown node type');
      });

      final service = ProfileService();
      expect(
        () => service.defaultNode(type: 'hologram', index: 0),
        throwsA(isA<Exception>()),
      );
    });

    test('validateProfile forwards payload and returns canonical JSON', () {
      const canonical = '{"themeId":"default","nodes":[]}';
      api.stubString(
        'crateFfiContentContentCustomProfileValidate',
        canonical,
      );

      final service = ProfileService();
      final result = service.validateProfile('{"nodes":[]}');

      expect(result, canonical);
      final inv = api
          .callsOf('crateFfiContentContentCustomProfileValidate')
          .single;
      expect(api.namedArg(inv, 'profileJson'), '{"nodes":[]}');
    });

    test('CustomProfile fromJson applies defaults for missing keys', () {
      final profile = CustomProfile.fromJson(const {});
      expect(profile.themeId, 'default');
      expect(profile.nodes, isEmpty);
    });

    test('CustomProfile toJson round-trips nodes', () {
      final node = CustomProfileNode.fromJson(const {
        'id': 'widget_1',
        'type': 'text_block',
        'styles': {},
        'position': {'row': 0, 'column': 0, 'order': 0},
        'properties': {'content': '', 'title': 'About Me'},
      });
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