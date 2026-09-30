// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert' show jsonEncode;

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/services/groups_service.dart';
import 'package:soshal_flutter/widgets/group_sidebar.dart';

import '../helpers/test_env.dart';

/// A custom role to seed: [id], [name] and its raw [permissions] blob, which is
/// the string the sidebar's parse memo is keyed on.
class RoleSpec {
  const RoleSpec(this.id, this.permissions, {this.name});

  final String id;
  final String permissions;
  final String? name;

  String get label => name ?? 'Role $id';
}

RoleSpec role(String id, String permissions) => RoleSpec(id, permissions);

/// The `GroupRole.fromJson` shape, with `permissions` carried through verbatim
/// -- the blob's exact text is what the memo is keyed on, so re-encoding it
/// would change what the test is about.
String roleJson(RoleSpec r) =>
    '{"id":"${r.id}","group_id":"g1","name":"${r.label}",'
    '"color":"#6b7280","position":1,'
    '"permissions":${jsonEncode(r.permissions)},"created_at":0}';

/// The member list and the permission matrix are the two unbounded parts of the
/// sidebar, and 6.6 made the first lazy and the second parsed once per role
/// instead of once per cell. These pin the properties that actually changed
/// rather than the pixels: a pixel assertion passes just as happily against the
/// eager version, which is the whole point.
void main() {
  final env = bootstrapTestEnv('test-group-sidebar');
  final api = env.$1;

  const groupJson = '{"id":"g1","name":"Soshal Devs","description":"d",'
      '"picture":"","owner":"pk-owner","members":0,"is_member":true,'
      '"role":"owner","is_private":false,"created_at":0}';

  /// A detail-bundle payload with [memberCount] members and [roles] custom
  /// roles. Seeded through the real `loadDetailBundle` path rather than by
  /// reaching into private state, so the test also covers the parse the sidebar
  /// then reads.
  String bundle({int memberCount = 0, List<RoleSpec>? roles}) {
    // Both keys, as the real bundle returns them: the header's count reads
    // `api.members.length` while the row list prefers `memberRoles`, so seeding
    // only one of them renders "Members (0)" above 5 000 rows.
    final pubkeys = [
      for (var i = 0; i < memberCount; i++) 'pk${i.toString().padLeft(4, '0')}',
    ];
    final memberRoles = [
      for (final pk in pubkeys) '{"pubkey":"$pk","role":"member"}',
    ];
    final encodedRoles = (roles ?? const []).map(roleJson).join(',');
    return '{"group":$groupJson,"members":${jsonEncode(pubkeys)},'
        '"memberRoles":[${memberRoles.join(',')}],'
        '"roles":[$encodedRoles],"rooms":[],"voiceChannels":[]}';
  }

  Future<GroupsService> loaded(
    WidgetTester tester, {
    int memberCount = 0,
    List<RoleSpec>? roles,
  }) async {
    final service = GroupsService();
    api.stubString('crateFfiGroupsGroupsGetDetailBundle',
        bundle(memberCount: memberCount, roles: roles));
    await service.loadDetailBundle('g1');
    return service;
  }

  /// [height] is a parameter because the sidebar is a single scroll view whose
  /// header holds the roles and the permission matrix: a test that asserts on
  /// the matrix has to make it reachable, and one that counts member rows must
  /// not, or the "only the visible window is built" assertion measures nothing.
  Future<void> pump(WidgetTester tester, GroupsService service,
      {double height = 2400}) async {
    tester.view.physicalSize = Size(1000, height);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      ChangeNotifierProvider<GroupsService>.value(
        value: service,
        child: MaterialApp(
          home: Scaffold(
            body: GroupSidebar(
              groupId: 'g1',
              isOwner: true,
              me: 'pk-owner',
              onChanged: () {},
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
  }

  group('member list laziness', () {
    testWidgets('a 5000-member server only builds the rows on screen',
        (tester) async {
      // If this ever times out or overflows, the list is eager again -- which is
      // what it was, and what the 5000-member case is here to catch.
      final service = await loaded(tester, memberCount: 5000);
      await pump(tester, service, height: 2400);

      expect(find.textContaining('Members (5000)'), findsOneWidget);
      final built = tester.widgetList(find.byType(ListTile)).length;
      expect(built, lessThan(300),
          reason: 'only the visible window plus cache extent may be built, '
              'but $built tiles were instantiated');
    });

    testWidgets('rows are built on demand and released when scrolled away',
        (tester) async {
      final service = await loaded(tester, memberCount: 200);
      await pump(tester, service, height: 2400);

      // Member titles are the only six-character strings starting `pk` on the
      // screen (`me` is `pk-owner`, and role labels are words), so this
      // isolates the member rows without depending on their order -- `sort` is
      // not stable, so "the first row is pk0000" would not hold.
      Set<String> visibleRows() => tester
          .widgetList<Text>(find.byType(Text))
          .map((t) => t.data ?? '')
          .where((t) => t.startsWith('pk') && t.length == 6)
          .toSet();

      final before = visibleRows();
      expect(before, isNotEmpty,
          reason: 'rows must be on screen for this to say anything');

      await tester.drag(find.byType(ListView).first, const Offset(0, -12000));
      await tester.pumpAndSettle();

      expect(visibleRows().intersection(before), isEmpty,
          reason: 'rows that scrolled off screen must no longer be built -- '
              'which is only possible if they were built on demand');
      expect(tester.takeException(), isNull);
    });

    testWidgets('an empty server still shows the empty state', (tester) async {
      await pump(tester, await loaded(tester));
      expect(find.text('No members yet.'), findsOneWidget);
      expect(find.textContaining('Members (0)'), findsOneWidget);
    });

    testWidgets('the last member is reachable by scrolling to the end',
        (tester) async {
      // `itemCount` is header + rows + 1. Too small silently drops the tail
      // member; too large renders a duplicate. Neither throws, so the only
      // way to see either is to scroll to the bottom and look -- which is what
      // this does, for the boundary sizes where an off-by-one bites.
      for (final n in [0, 1, 3, 40]) {
        final service = await loaded(tester, memberCount: n);
        await pump(tester, service, height: 2400);
        await tester.drag(find.byType(ListView).first, const Offset(0, -40000));
        await tester.pumpAndSettle();

        if (n > 0) {
          expect(find.text('pk${(n - 1).toString().padLeft(4, '0')}'),
              findsOneWidget,
              reason: 'the last of $n member rows must be reachable');
        }
        expect(tester.takeException(), isNull, reason: 'memberCount=$n');
      }
    });
  });

  group('permission matrix parsing', () {
    testWidgets('renders a check per granted permission and a cross otherwise',
        (tester) async {
      await pump(tester,
          await loaded(tester, roles: [role('r0', '["canPost","canChat"]')]));
      expect(find.byIcon(Icons.check), findsNWidgets(2));
      expect(find.byIcon(Icons.remove),
          findsNWidgets(groupPermissionKeys.length - 2));
    });

    testWidgets('a role with no permissions renders all crosses',
        (tester) async {
      await pump(tester, await loaded(tester, roles: [role('none', '[]')]));
      expect(find.byIcon(Icons.check), findsNothing);
      expect(
          find.byIcon(Icons.remove), findsNWidgets(groupPermissionKeys.length));
    });

    testWidgets('a map-shaped permissions blob is read as booleans',
        (tester) async {
      await pump(
          tester,
          await loaded(tester,
              roles: [role('m0', '{"canChat":true,"canPost":false}')]));
      // Only `canChat` is `true`; a list-shaped read of a map would grant both
      // or neither.
      expect(find.byIcon(Icons.check), findsNWidgets(1));
    });

    testWidgets('an edited role is not served from a stale memo entry',
        (tester) async {
      final service =
          await loaded(tester, roles: [role('r0', '["canPost","canChat"]')]);
      await pump(tester, service);
      expect(find.byIcon(Icons.check), findsNWidgets(2));

      // The memo is keyed on the raw blob, so a changed blob must miss. This is
      // the case a memo keyed on the role id, or one never invalidated, gets
      // wrong -- and it would show as permissions that never take effect.
      api.stubString('crateFfiGroupsGroupsGetDetailBundle',
          bundle(roles: [role('r0', '["canPost"]')]));
      await service.loadDetailBundle('g1');
      await tester.pumpAndSettle();

      expect(find.byIcon(Icons.check), findsNWidgets(1),
          reason: 'the new blob grants one permission, not two');
    });

    testWidgets('a deleted role disappears from the matrix', (tester) async {
      final service = await loaded(tester, roles: [
        role('r0', '["canPost"]'),
        role('r1', '["canChat"]'),
      ]);
      await pump(tester, service);
      expect(find.byIcon(Icons.check), findsNWidgets(2));

      api.stubString('crateFfiGroupsGroupsGetDetailBundle',
          bundle(roles: [role('r0', '["canPost"]')]));
      await service.loadDetailBundle('g1');
      await tester.pumpAndSettle();

      expect(find.byIcon(Icons.check), findsNWidgets(1));
      expect(find.text('Role r1'), findsNothing);
    });

    testWidgets("each row shows its own role's permissions", (tester) async {
      // Catches the per-role parallel list going out of step with the role
      // rows. With an identical blob on every role the assertion holds for any
      // misalignment, including a fixed offset of one.
      await pump(
          tester,
          await loaded(tester, roles: [
            role('a', '["canPost"]'),
            role('b', '["canPost","canChat"]'),
            role('c', '["canChat"]'),
          ]));

      // Rows are stacked, so checks sharing a y belong to the same role. 1, 2
      // then 1 is the expected top-to-bottom shape; a shift by one role would
      // give 2, 1, 1 or 1, 1, 2.
      final perRow = <int, int>{};
      for (final icon in tester.widgetList<Icon>(find.byIcon(Icons.check))) {
        final y = tester.getCenter(find.byWidget(icon)).dy.round();
        perRow[y] = (perRow[y] ?? 0) + 1;
      }
      // Top to bottom, not sorted by count: the shape *is* the claim.
      final byRow = perRow.keys.toList()..sort();
      expect(byRow.map((y) => perRow[y]!).toList(), [1, 2, 1],
          reason: 'one check, then two, then one -- got $perRow');
    });

    testWidgets('30 roles render, and the matrix still shows 30 checks',
        (tester) async {
      // This is the shape 6.6's hoist exists for: 30 roles is 600 permission
      // cells, which used to be 600 `jsonDecode` calls per build. It also
      // checks the per-role parallel list stays aligned with the role rows --
      // a misaligned index would shift every row's permissions by one.
      await pump(
          tester,
          await loaded(tester,
              roles: [for (var i = 0; i < 30; i++) role('r$i', '["canPost"]')]),
          height: 12000);
      expect(tester.takeException(), isNull);
      expect(find.byIcon(Icons.check), findsNWidgets(30),
          reason: 'one granted permission per role, so one check per row');
    });
  });
}
