import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/screens/marketplace_screen.dart';
import 'package:soshal_flutter/services/marketplace_service.dart';
import 'package:soshal_flutter/services/session_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-marketplace');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  const sessionJson =
      '{"active_pubkey":"pk123","accounts":[{"pubkey":"pk123",'
      '"npub":"npub1abc","last_used":0,"relay_list":[]}]}';

  const listingJson =
      '{"id":"l1","seller_pubkey":"pkS","seller_name":"","title":'
      '"Vintage Guitar","description":"nice","images":[],"price":250,'
      '"currency":"sats","category":"music","condition":"new",'
      '"shipping_available":true,"created_at":0,"updated_at":0,"status":"active"}';

  Future<void> pumpScreen(WidgetTester tester) async {
    tester.view.physicalSize = const Size(800, 2400);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    final session = SessionService();
    api.stubString('crateFfiSessionSessionLoad', sessionJson);
    await session.loadSession();

    // Default load stubs — only when the test did not register its own.
    void stubDefault(String method, String result) {
      if (!api.handlers.containsKey(Symbol(method))) {
        api.stubString(method, result);
      }
    }

    stubDefault('crateFfiMarketplaceMarketplaceFetchListings', '[]');
    stubDefault('crateFfiMarketplaceMarketplaceFetchSellerListings', '[]');
    stubDefault('crateFfiMarketplaceMarketplaceFetchSellerOrders', '[]');

    await tester.pumpWidget(
      MultiProvider(
        providers: [
          ChangeNotifierProvider<SessionService>.value(value: session),
          ChangeNotifierProvider(create: (_) => MarketplaceService()),
        ],
        child: const MaterialApp(home: MarketplaceScreen()),
      ),
    );
    await tester.pumpAndSettle();
  }

  testWidgets('empty state shows no listings yet with tabs', (tester) async {
    await pumpScreen(tester);

    expect(find.text('No listings yet'), findsOneWidget);
    expect(find.byTooltip('Create listing'), findsOneWidget);
    expect(find.text('Browse'), findsOneWidget);
    expect(find.text('Orders'), findsOneWidget);
    expect(find.text('Mine'), findsOneWidget);
  });

  testWidgets('renders listing rows and triggers load fns', (tester) async {
    api.stubString(
        'crateFfiMarketplaceMarketplaceFetchListings', '[$listingJson]');

    await pumpScreen(tester);

    expect(find.text('Vintage Guitar'), findsOneWidget);
    expect(find.textContaining('250 sats'), findsOneWidget);
    expect(find.text('New'), findsOneWidget);
    expect(find.widgetWithText(FilledButton, 'Buy'), findsOneWidget);
    expect(api.callCount('crateFfiMarketplaceMarketplaceFetchListings'), 1);
    expect(
        api.callCount('crateFfiMarketplaceMarketplaceFetchSellerListings'), 1);
    expect(api.callCount('crateFfiMarketplaceMarketplaceFetchSellerOrders'), 1);
  });

  testWidgets('load failure shows empty state without crash', (tester) async {
    api.stub('crateFfiMarketplaceMarketplaceFetchListings', (_) {
      throw Exception('db locked');
    });

    await pumpScreen(tester);

    expect(find.text('No listings yet'), findsOneWidget);
  });

  testWidgets('create dialog posts listing with entered fields',
      (tester) async {
    api.stubString('crateFfiMarketplaceMarketplaceCreateListing', 'evt1');

    await pumpScreen(tester);

    await tester.tap(find.byType(FloatingActionButton));
    await tester.pumpAndSettle();
    expect(find.text('Create listing'), findsOneWidget);

    final fields = find.descendant(
        of: find.byType(AlertDialog), matching: find.byType(TextField));
    await tester.enterText(fields.at(0), 'Vintage Amp');
    await tester.enterText(fields.at(1), 'tube amp');
    await tester.enterText(fields.at(2), '2500');
    await tester.tap(find.widgetWithText(FilledButton, 'Post'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiMarketplaceMarketplaceCreateListing'), 1);
    final inv =
        api.callsOf('crateFfiMarketplaceMarketplaceCreateListing').single;
    expect(api.namedArg(inv, 'sellerPubkey'), 'pk123');
    expect(api.namedArg(inv, 'title'), 'Vintage Amp');
    expect(api.namedArg(inv, 'description'), 'tube amp');
    expect(api.namedArg(inv, 'price'), BigInt.from(2500));
    expect(api.namedArg(inv, 'currency'), 'sats');
    expect(api.namedArg(inv, 'condition'), 'new');
    expect(api.namedArg(inv, 'imagesJson'), '[]');
    expect(api.callCount('crateFfiMarketplaceMarketplaceFetchListings'), 2);
  });

  testWidgets('create failure surfaces snackbar', (tester) async {
    api.stub('crateFfiMarketplaceMarketplaceCreateListing', (_) {
      throw Exception('publish failed');
    });

    await pumpScreen(tester);

    await tester.tap(find.byType(FloatingActionButton));
    await tester.pumpAndSettle();
    await tester.enterText(
        find
            .descendant(
                of: find.byType(AlertDialog),
                matching: find.byType(TextField))
            .at(0),
        'Broken');
    await tester.tap(find.widgetWithText(FilledButton, 'Post'));
    await tester.pumpAndSettle();

    expect(find.textContaining('Create failed'), findsOneWidget);
    await tester.pump(const Duration(seconds: 5));
  });

  testWidgets('buy creates order and escrow with listing args',
      (tester) async {
    api.stubString(
        'crateFfiMarketplaceMarketplaceFetchListings', '[$listingJson]');
    api.stubString('crateFfiMarketplaceMarketplaceCreateOrder', 'o1');
    api.stubString('crateFfiMarketplaceMarketplaceCreateEscrow', 'esc1');
    api.stubString('crateFfiDbDbGetEscrowsByParticipant', '[]');

    await pumpScreen(tester);

    await tester.tap(find.widgetWithText(FilledButton, 'Buy'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiMarketplaceMarketplaceCreateOrder'), 1);
    final orderInv =
        api.callsOf('crateFfiMarketplaceMarketplaceCreateOrder').single;
    expect(api.namedArg(orderInv, 'listingId'), 'l1');
    expect(api.namedArg(orderInv, 'buyerPubkey'), 'pk123');
    expect(api.namedArg(orderInv, 'sellerPubkey'), 'pkS');

    expect(find.text('Escrow'), findsOneWidget);
    // Dismiss the "Order created" snackbar so the escrow snackbar can show.
    await tester.pump(const Duration(seconds: 5));
    await tester.tap(find.widgetWithText(FilledButton, 'Create escrow'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiMarketplaceMarketplaceCreateEscrow'), 1);
    final escrowInv =
        api.callsOf('crateFfiMarketplaceMarketplaceCreateEscrow').single;
    expect(api.namedArg(escrowInv, 'orderId'), 'o1');
    expect(api.namedArg(escrowInv, 'buyerPubkey'), 'pk123');
    expect(api.namedArg(escrowInv, 'sellerPubkey'), 'pkS');
    expect(api.namedArg(escrowInv, 'amount'), BigInt.from(250));
    expect(find.textContaining('Escrow esc1 created'), findsOneWidget);
    await tester.pump(const Duration(seconds: 5));
  });

  testWidgets('listing detail dialog renders meta and escrow panel',
      (tester) async {
    api.stubString(
        'crateFfiMarketplaceMarketplaceFetchListings', '[$listingJson]');
    api.stubString('crateFfiMarketplaceMarketplaceGetListing', listingJson);
    api.stubString('crateFfiMarketplaceMarketplaceGetEscrowByListing', 'null');
    api.stub('crateFfiMarketplaceMarketplaceListingRating', (_) => 4.5);
    api.stubString('crateFfiMarketplaceMarketplaceListingReviews', '[]');

    await pumpScreen(tester);

    await tester.tap(find.text('Vintage Guitar'));
    await tester.pumpAndSettle();

    final dialog = find.byType(AlertDialog);
    expect(
        find.descendant(of: dialog, matching: find.text('Vintage Guitar')),
        findsOneWidget);
    expect(
        find.descendant(
            of: dialog, matching: find.textContaining('250 sats')),
        findsWidgets);
    expect(
        find.descendant(of: dialog, matching: find.textContaining('Seller:')),
        findsOneWidget);
    expect(
        find.descendant(
            of: dialog, matching: find.text('Open Escrow Panel')),
        findsOneWidget);
    expect(
        find.descendant(
            of: dialog, matching: find.text('4.5 · 0 reviews')),
        findsOneWidget);
    expect(
        find.descendant(of: dialog, matching: find.text('Close')),
        findsOneWidget);

    await tester.tap(find.text('Open Escrow Panel'));
    await tester.pumpAndSettle();
    expect(find.text('No escrow for this listing yet.'), findsOneWidget);
    await tester.tap(find.widgetWithText(TextButton, 'Close'));
    await tester.pumpAndSettle();
  });

  testWidgets('mine tab edit updates listing and reloads', (tester) async {
    api.stubString(
        'crateFfiMarketplaceMarketplaceFetchSellerListings', '[$listingJson]');
    api.stubBool('crateFfiMarketplaceMarketplaceUpdateListing', true);

    await pumpScreen(tester);

    await tester.tap(find.text('Mine'));
    await tester.pumpAndSettle();
    expect(find.text('Vintage Guitar'), findsOneWidget);

    await tester.tap(find.byTooltip('Edit'));
    await tester.pumpAndSettle();
    expect(find.text('Edit listing'), findsOneWidget);

    final fields = find.descendant(
        of: find.byType(AlertDialog), matching: find.byType(TextField));
    await tester.enterText(fields.at(0), 'Vintage Guitar Pro');
    await tester.tap(find.widgetWithText(FilledButton, 'Save'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiMarketplaceMarketplaceUpdateListing'), 1);
    final inv =
        api.callsOf('crateFfiMarketplaceMarketplaceUpdateListing').single;
    expect(api.namedArg(inv, 'listingId'), 'l1');
    expect(api.namedArg(inv, 'sellerPubkey'), 'pk123');
    expect(api.namedArg(inv, 'title'), 'Vintage Guitar Pro');
    expect(api.namedArg(inv, 'price'), BigInt.from(250));
    expect(find.text('Listing updated'), findsOneWidget);
    expect(api.callCount('crateFfiMarketplaceMarketplaceFetchListings'), 2);
    await tester.pump(const Duration(seconds: 5));
  });

  testWidgets('mine tab delete calls bridge with listing and pubkey',
      (tester) async {
    api.stubString(
        'crateFfiMarketplaceMarketplaceFetchSellerListings', '[$listingJson]');
    api.stubBool('crateFfiMarketplaceMarketplaceDeleteListing', true);

    await pumpScreen(tester);

    await tester.tap(find.text('Mine'));
    await tester.pumpAndSettle();

    await tester.tap(find.byTooltip('Delete'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiMarketplaceMarketplaceDeleteListing'), 1);
    final inv =
        api.callsOf('crateFfiMarketplaceMarketplaceDeleteListing').single;
    expect(api.namedArg(inv, 'listingId'), 'l1');
    expect(api.namedArg(inv, 'sellerPubkey'), 'pk123');
  });

  testWidgets('trending and category chips filter listings', (tester) async {
    api.stubString(
        'crateFfiMarketplaceMarketplaceGetTrending', '[$listingJson]');
    api.stubString(
        'crateFfiMarketplaceMarketplaceGetByCategory', '[$listingJson]');

    await pumpScreen(tester);

    await tester.tap(find.text('🔥 Trending'));
    await tester.pumpAndSettle();
    expect(api.callCount('crateFfiMarketplaceMarketplaceGetTrending'), 1);

    await tester.tap(find.text('electronics'));
    await tester.pumpAndSettle();
    expect(api.callCount('crateFfiMarketplaceMarketplaceGetByCategory'), 1);
    final inv = api.callsOf('crateFfiMarketplaceMarketplaceGetByCategory').single;
    expect(api.namedArg(inv, 'category'), 'electronics');
  });

  testWidgets('search submits query to bridge', (tester) async {
    api.stubString(
        'crateFfiMarketplaceMarketplaceSearch', '[$listingJson]');

    await pumpScreen(tester);

    await tester.enterText(find.byType(TextField), 'guitar');
    await tester.testTextInput.receiveAction(TextInputAction.search);
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiMarketplaceMarketplaceSearch'), 1);
    final inv = api.callsOf('crateFfiMarketplaceMarketplaceSearch').single;
    expect(api.namedArg(inv, 'query'), 'guitar');
    expect(api.callCount('crateFfiMarketplaceMarketplaceFetchListings'), 1);
  });
}