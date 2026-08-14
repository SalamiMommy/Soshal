// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/marketplace_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

Map<String, dynamic> listingJson(
  String id, {
  String status = 'active',
  String seller = 'pk-seller',
  int price = 5000,
}) =>
    {
      'id': id,
      'seller_pubkey': seller,
      'seller_name': 'Seller $id',
      'title': 'Listing $id',
      'description': 'desc $id',
      'images': ['img-$id'],
      'price': price,
      'currency': 'BTC',
      'category': 'art',
      'condition': 'new',
      'shipping_available': true,
      'created_at': 1700000000,
      'updated_at': 1700000100,
      'status': status,
    };

Map<String, dynamic> orderJson(String id, String listingId) => {
      'id': id,
      'listing_id': listingId,
      'buyer_pubkey': 'pk-buyer',
      'seller_pubkey': 'pk-seller',
      'status': 'pending',
      'amount': 5000,
      'created_at': 1700000200,
    };

void main() {
  final env = bootstrapTestEnv('test-marketplace');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('MarketplaceService', () {
    test('fetchListings parses, caps at 100, forwards limit/offset',
        () async {
      final mp = MarketplaceService();
      api.stub(
        'crateFfiMarketplaceMarketplaceFetchListings',
        (_) => jsonEncode(
          List.generate(120, (i) => listingJson('l-$i', price: i)),
        ),
      );

      final listings = await mp.fetchListings(limit: 150, offset: 0);
      expect(listings.length, 100, reason: 'capped at 100');
      expect(mp.listings.length, 100);
      expect(mp.listings.first.title, 'Listing l-0');
      expect(mp.listings.first.price, 0);
      expect(mp.listings.first.sellerName, 'Seller l-0');
      expect(mp.listings.first.status, 'active');
      expect(mp.listings.first.priceLabel, '0 BTC');
      final inv =
          api.callsOf('crateFfiMarketplaceMarketplaceFetchListings').single;
      expect(api.namedArg(inv, 'limit'), 150);
      expect(api.namedArg(inv, 'offset'), 0);
    });

    test('search, byCategory, trending, sellerListings forward args',
        () async {
      final mp = MarketplaceService();
      final one = jsonEncode([listingJson('l-1')]);
      api.stubString('crateFfiMarketplaceMarketplaceSearch', one);
      api.stubString('crateFfiMarketplaceMarketplaceGetByCategory', one);
      api.stubString('crateFfiMarketplaceMarketplaceGetTrending', one);
      api.stubString('crateFfiMarketplaceMarketplaceFetchSellerListings', one);

      await mp.search('gadgets', limit: 20);
      var inv =
          api.callsOf('crateFfiMarketplaceMarketplaceSearch').single;
      expect(api.namedArg(inv, 'query'), 'gadgets');
      expect(api.namedArg(inv, 'limit'), 20);

      await mp.byCategory('art');
      inv = api.callsOf('crateFfiMarketplaceMarketplaceGetByCategory').single;
      expect(api.namedArg(inv, 'category'), 'art');
      expect(api.namedArg(inv, 'limit'), 50);

      await mp.trending();
      inv = api.callsOf('crateFfiMarketplaceMarketplaceGetTrending').single;
      expect(api.namedArg(inv, 'limit'), 50);

      await mp.sellerListings('pk-seller');
      inv = api
          .callsOf('crateFfiMarketplaceMarketplaceFetchSellerListings')
          .single;
      expect(api.namedArg(inv, 'sellerPubkey'), 'pk-seller');
    });

    test('getListing sets current and parses status field', () async {
      final mp = MarketplaceService();
      var notified = 0;
      mp.addListener(() => notified++);
      api.stubString(
        'crateFfiMarketplaceMarketplaceGetListing',
        jsonEncode(listingJson('l-9', status: 'sold')),
      );

      final listing = await mp.getListing('l-9');
      expect(listing.id, 'l-9');
      expect(mp.current?.title, 'Listing l-9');
      expect(mp.current?.status, 'sold');
      expect(mp.current?.images, ['img-l-9']);
      expect(notified, 1);
      final inv =
          api.callsOf('crateFfiMarketplaceMarketplaceGetListing').single;
      expect(api.namedArg(inv, 'listingId'), 'l-9');
    });

    test('createListing forwards every field as named args', () async {
      final mp = MarketplaceService();
      api.stubString('crateFfiMarketplaceMarketplaceCreateListing', 'ev-1');

      final id = await mp.createListing('pk-seller', 'Title', 'Desc', 10000,
          'BTC', 'art', 'new', const ['a.jpg', 'b.jpg'], true);
      expect(id, 'ev-1');

      final inv =
          api.callsOf('crateFfiMarketplaceMarketplaceCreateListing').single;
      expect(api.namedArg(inv, 'sellerPubkey'), 'pk-seller');
      expect(api.namedArg(inv, 'title'), 'Title');
      expect(api.namedArg(inv, 'description'), 'Desc');
      expect(api.namedArg(inv, 'price'), BigInt.from(10000));
      expect(api.namedArg(inv, 'currency'), 'BTC');
      expect(api.namedArg(inv, 'category'), 'art');
      expect(api.namedArg(inv, 'condition'), 'new');
      expect(api.namedArg(inv, 'imagesJson'), '["a.jpg","b.jpg"]');
      expect(api.namedArg(inv, 'shippingAvailable'), isTrue);
    });

    test('updateListing and deleteListing pass ids and price', () async {
      final mp = MarketplaceService();
      api.stubBool('crateFfiMarketplaceMarketplaceUpdateListing', true);
      api.stubBool('crateFfiMarketplaceMarketplaceDeleteListing', true);

      expect(
        await mp.updateListing('l-1', 'pk-seller', 'New', 'Newdesc', 8000),
        isTrue,
      );
      var inv = api
          .callsOf('crateFfiMarketplaceMarketplaceUpdateListing')
          .single;
      expect(api.namedArg(inv, 'listingId'), 'l-1');
      expect(api.namedArg(inv, 'price'), BigInt.from(8000));

      expect(await mp.deleteListing('l-1', 'pk-seller'), isTrue);
      inv =
          api.callsOf('crateFfiMarketplaceMarketplaceDeleteListing').single;
      expect(api.namedArg(inv, 'listingId'), 'l-1');
      expect(api.namedArg(inv, 'sellerPubkey'), 'pk-seller');
    });

    test('orders: create and fetch fill orders cache', () async {
      final mp = MarketplaceService();
      api.stubString('crateFfiMarketplaceMarketplaceCreateOrder', 'ord-1');
      api.stubString(
        'crateFfiMarketplaceMarketplaceFetchBuyerOrders',
        jsonEncode([orderJson('ord-1', 'l-1')]),
      );
      api.stubString(
        'crateFfiMarketplaceMarketplaceFetchSellerOrders',
        jsonEncode([orderJson('ord-2', 'l-2')]),
      );

      expect(
        await mp.createOrder('l-1', 'pk-buyer', 'pk-seller'),
        'ord-1',
      );
      var inv =
          api.callsOf('crateFfiMarketplaceMarketplaceCreateOrder').single;
      expect(api.namedArg(inv, 'listingId'), 'l-1');
      expect(api.namedArg(inv, 'buyerPubkey'), 'pk-buyer');

      await mp.buyerOrders('pk-buyer');
      expect(mp.orders.single.status, 'pending');
      expect(mp.orders.single.amount, 5000);
      expect(mp.orderForListing('l-1')?.id, 'ord-1');
      expect(mp.orderForListing('missing'), isNull);
      inv =
          api.callsOf('crateFfiMarketplaceMarketplaceFetchBuyerOrders').single;
      expect(api.namedArg(inv, 'buyerPubkey'), 'pk-buyer');

      await mp.sellerOrders('pk-seller');
      expect(mp.orders.single.listingId, 'l-2');
    });

    test('escrow lifecycle: create, fetch, null, supportsEscrow', () async {
      final mp = MarketplaceService();
      api.stubString('crateFfiMarketplaceMarketplaceCreateEscrow', 'esc-1');
      api.stubString(
        'crateFfiMarketplaceMarketplaceGetEscrowByListing',
        'null',
      );
      api.stubString(
        'crateFfiMarketplaceMarketplaceGetContent',
        '{"escrowEnabled":true}',
      );

      expect(
        await mp.createEscrow('ord-1', 'pk-buyer', 'pk-seller', 10000),
        'esc-1',
      );
      final inv =
          api.callsOf('crateFfiMarketplaceMarketplaceCreateEscrow').single;
      expect(api.namedArg(inv, 'orderId'), 'ord-1');
      expect(api.namedArg(inv, 'amount'), BigInt.from(10000));

      expect(await mp.getEscrowByListing('l-1'), isNull);
      api.stubString(
        'crateFfiMarketplaceMarketplaceGetEscrowByListing',
        jsonEncode({
          'id': 'esc-1',
          'listing_id': 'l-1',
          'buyer_pubkey': 'pk-buyer',
          'seller_pubkey': 'pk-seller',
          'amount_msats': 10000000,
          'currency': 'BTC',
          'status': 'created',
          'escrow_note': '',
          'created_at': 1700000000,
          'updated_at': 1700000000,
        }),
      );
      final escrow = await mp.getEscrowByListing('l-1');
      expect(escrow?.amountMsats, 10000000);
      expect(escrow?.status, 'created');
      expect(escrow?.isTerminal, isFalse);

      expect(await mp.supportsEscrow('l-1'), isTrue);
      final contentInv =
          api.callsOf('crateFfiMarketplaceMarketplaceGetContent').single;
      expect(api.namedArg(contentInv, 'listingId'), 'l-1');
    });

    test('reviewListing and listingReviews forward args', () async {
      final mp = MarketplaceService();
      api.stubBool('crateFfiMarketplaceMarketplaceReviewListing', true);
      api.stubString(
        'crateFfiMarketplaceMarketplaceListingReviews',
        jsonEncode([
          {'reviewer': 'pk-r', 'rating': 5, 'text': 'great'}
        ]),
      );
      api.stub('crateFfiMarketplaceMarketplaceListingRating', (_) => 4.5);

      expect(
        await mp.reviewListing(
            listingId: 'l-1', reviewerPubkey: 'pk-r', rating: 5, text: 'great'),
        isTrue,
      );
      final inv =
          api.callsOf('crateFfiMarketplaceMarketplaceReviewListing').single;
      expect(api.namedArg(inv, 'listingId'), 'l-1');
      expect(api.namedArg(inv, 'reviewerPubkey'), 'pk-r');
      expect(api.namedArg(inv, 'rating'), 5);
      expect(api.namedArg(inv, 'text'), 'great');

      final reviews = await mp.listingReviews('l-1', limit: 10);
      expect(reviews.single['rating'], 5);
      expect(await mp.listingRating('l-1'), 4.5);
      final ratingInv =
          api.callsOf('crateFfiMarketplaceMarketplaceListingRating').single;
      expect(api.namedArg(ratingInv, 'listingId'), 'l-1');
    });

    test('polls: create, vote, close, get, hasVoted', () async {
      final mp = MarketplaceService();
      api.stubString(
        'crateFfiMarketplaceMarketplacePollCreate',
        '{"id":"poll-1","question":"q?"}',
      );
      api.stubBool('crateFfiMarketplaceMarketplacePollVote', true);
      api.stubBool('crateFfiMarketplaceMarketplacePollClose', true);
      api.stubString(
        'crateFfiMarketplaceMarketplacePollGet',
        '{"id":"poll-1","votes":[3,7]}',
      );
      api.stubBool('crateFfiMarketplaceMarketplacePollHasVoted', true);

      final created = await mp.pollCreate(
        userPubkey: 'pk-u',
        question: 'q?',
        optionsJson: '["a","b"]',
      );
      expect(created?['id'], 'poll-1');
      var inv =
          api.callsOf('crateFfiMarketplaceMarketplacePollCreate').single;
      expect(api.namedArg(inv, 'userPubkey'), 'pk-u');
      expect(api.namedArg(inv, 'expiresInHours'), 168);

      expect(
        await mp.pollVote(pollId: 'poll-1', voterPubkey: 'pk-v', optionIndex: 1),
        isTrue,
      );
      inv = api.callsOf('crateFfiMarketplaceMarketplacePollVote').single;
      expect(api.namedArg(inv, 'optionIndex'), 1);

      expect(await mp.pollClose('poll-1', 'pk-u'), isTrue);
      final poll = await mp.pollGet('poll-1');
      expect(poll?['votes'], [3, 7]);
      expect(await mp.pollHasVoted('poll-1', 'pk-v'), isTrue);
    });

    test('fetch error sets lastError and rethrows', () async {
      final mp = MarketplaceService();
      api.stub('crateFfiMarketplaceMarketplaceFetchListings',
          (_) => throw Exception('mp down'));
      await expectLater(mp.fetchListings(), throwsException);
      expect(mp.lastError, contains('mp down'));
      expect(mp.listings, isEmpty);
    });

    test('create error sets lastError and rethrows; review swallows',
        () async {
      final mp = MarketplaceService();
      api.stub('crateFfiMarketplaceMarketplaceCreateListing',
          (_) => throw Exception('create boom'));
      await expectLater(
        mp.createListing('pk', 't', 'd', 1, 'BTC', 'a', 'new', const [], false),
        throwsException,
      );
      expect(mp.lastError, contains('create boom'));

      api.stub('crateFfiMarketplaceMarketplaceReviewListing',
          (_) => throw Exception('review boom'));
      expect(
        await mp.reviewListing(
            listingId: 'l-1', reviewerPubkey: 'pk-r', rating: 1),
        isFalse,
      );
      expect(mp.lastError, contains('review boom'));
    });
  });
}