import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/screens/edit_profile_screen.dart';
import 'package:soshal_flutter/screens/friends_screen.dart';
import 'package:soshal_flutter/screens/language_screen.dart';
import 'package:soshal_flutter/screens/search_screen.dart';
import 'package:soshal_flutter/services/friends_service.dart';
import 'package:soshal_flutter/services/messaging_service.dart';
import 'package:soshal_flutter/services/search_service.dart';
import 'package:soshal_flutter/services/session_service.dart';
import 'package:soshal_flutter/services/settings_service.dart';

import '../helpers/test_env.dart';

late FakeApi api;

class FakeSearchService extends SearchService {
  @override
  Future<List<SearchResultItem>> searchPosts(String query,
      {int limit = 50}) async {
    await Future<void>.delayed(Duration.zero);
    notifyListeners();
    return [];
  }
}

class FakeFriendsService extends FriendsService {
  @override
  Future<List<String>> fetchSuggestions() async {
    await Future<void>.delayed(Duration.zero);
    notifyListeners();
    return [];
  }
}

void main() {
  final env = bootstrapTestEnv('test-profile-edit-screens');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  const sessionJson =
      '{"active_pubkey":"pk123","accounts":[{"pubkey":"pk123",'
      '"npub":"npub1abc","last_used":0,'
      '"relay_list":["wss://relay.example.com"]}]}';

  Future<SessionService> pump(WidgetTester tester, Widget child,
      {bool withSession = false}) async {
    tester.view.physicalSize = const Size(900, 2600);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    final session = SessionService();
    if (withSession) {
      api.stubString('crateFfiSessionSessionLoad', sessionJson);
      await session.loadSession();
    }
    await tester.pumpWidget(MultiProvider(
      providers: [
        ChangeNotifierProvider<SessionService>.value(value: session),
        ChangeNotifierProvider(create: (_) => IdentityService()),
        ChangeNotifierProvider<FriendsService>.value(
          value: FakeFriendsService(),
        ),
        ChangeNotifierProvider<SearchService>.value(
          value: FakeSearchService(),
        ),
        ChangeNotifierProvider(create: (_) => SettingsService()),
      ],
      child: MaterialApp(home: child),
    ));
    await tester.pumpAndSettle();
    return session;
  }

  testWidgets('edit profile screen renders empty form with account',
      (tester) async {
    api.stubString('crateFfiIdentityIdentityGetProfile', '{}');
    await pump(tester, const EditProfileScreen(), withSession: true);

    expect(find.text('Edit Profile'), findsOneWidget);
    expect(find.text('Save'), findsOneWidget);
    expect(find.text('Display name'), findsOneWidget);
    expect(find.text('About'), findsOneWidget);
    expect(find.text('NIP-05 identifier'), findsOneWidget);
    expect(api.callCount('crateFfiIdentityIdentityGetProfile'), 1);
  });

  testWidgets('edit profile screen keeps form on load failure',
      (tester) async {
    api.stub('crateFfiIdentityIdentityGetProfile', (_) {
      throw Exception('relay down');
    });
    await pump(tester, const EditProfileScreen(), withSession: true);

    expect(find.text('Edit Profile'), findsOneWidget);
    expect(find.text('Display name'), findsOneWidget);
  });

  testWidgets('friends screen without account renders discovery panel',
      (tester) async {
    await pump(tester, const FriendsScreen());

    expect(find.text('Friends'), findsOneWidget);
    expect(find.text('People you may know'), findsOneWidget);
    expect(find.text('Add friend'), findsOneWidget);
    expect(
      find.text('No suggestions yet — friend discovery arrives with the backend.'),
      findsOneWidget,
    );
  });

  testWidgets('friends screen with account shows contacts section',
      (tester) async {
    await pump(tester, const FriendsScreen(), withSession: true);

    expect(find.text('Friends'), findsOneWidget);
    expect(find.text('My contacts (0)'), findsOneWidget);
    expect(find.text('Refresh from relays'), findsOneWidget);
  });

  testWidgets('search screen renders search bar and trending sections', (tester) async {
    api.stubString('crateFfiDbDbGetTrendingHashtags', '[]');
    api.stubString('crateFfiSearchSearchTrendingHashtags', '[]');
    api.stubString('crateFfiSearchSearchTrendingProfiles', '[]');
    await pump(tester, const SearchScreen());
    await tester.pumpAndSettle();

    expect(find.byType(TextField), findsOneWidget);
    expect(find.text('Trending hashtags'), findsOneWidget);
    expect(find.text('Trending profiles'), findsOneWidget);
  });

  testWidgets('language screen renders language options', (tester) async {
    api.stubString('crateFfiDbDbGetSetting', '');
    await pump(tester, const LanguageScreen());

    expect(find.text('Language'), findsOneWidget);
    expect(find.text('English'), findsOneWidget);
    expect(find.text('Deutsch'), findsOneWidget);
    expect(find.text('Français'), findsOneWidget);
    expect(find.text('Español'), findsOneWidget);
    expect(find.text('日本語'), findsOneWidget);
  });
}