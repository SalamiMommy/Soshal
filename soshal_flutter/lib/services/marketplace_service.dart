// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import '../utils/json_ext.dart';
import '../utils/offthread.dart';
import 'error_log.dart';

/// Marketplace Service
/// NIP-15 style listings, orders and escrow through the bridge.
class MarketplaceService extends ChangeNotifier
    with LastErrorMixin, DeferredNotify {
  List<ListingInfo> _listings = [];
  bool _listingsLoading = false;
  ListingInfo? _current;
  List<OrderInfo> _orders = [];

  List<ListingInfo> get listings => _listings;
  bool get listingsLoading => _listingsLoading;
  ListingInfo? get current => _current;
  List<OrderInfo> get orders => _orders;

  /// Clear all account-scoped state on account switch so Account B never
  /// sees Account A's cached listings, current listing, or orders.
  void resetForAccountSwitch() {
    _listings = [];
    _listingsLoading = false;
    _current = null;
    _orders = [];
    clearLastError();
    notifyListeners();
  }

  Future<List<ListingInfo>> fetchListings(
      {int limit = 50, int offset = 0}) async {
    _listingsLoading = true;
    notifyDeferred();
    try {
      final result = await _decode(
        () => RustLib.instance.api.crateFfiMarketplaceMarketplaceFetchListings(
          limit: limit,
          offset: offset,
          audience: 'public',
        ),
      );
      _listings = result;
      _listingsLoading = false;
      clearLastError();
      notifyDeferred();
      return _listings;
    } catch (e, st) {
      _listingsLoading = false;
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<List<ListingInfo>> search(String query, {int limit = 50}) async {
    return _decode(
      () => RustLib.instance.api.crateFfiMarketplaceMarketplaceSearch(
        query: query,
        limit: limit,
        audience: 'public',
      ),
    );
  }

  Future<List<ListingInfo>> sellerListings(String sellerPubkey) async {
    return _decode(
      () => RustLib.instance.api
          .crateFfiMarketplaceMarketplaceFetchSellerListings(
        sellerPubkey: sellerPubkey,
      ),
    );
  }

  Future<List<ListingInfo>> byCategory(String category,
      {int limit = 50}) async {
    return _decode(
      () => RustLib.instance.api.crateFfiMarketplaceMarketplaceGetByCategory(
        category: category,
        limit: limit,
        audience: 'public',
      ),
    );
  }

  Future<List<ListingInfo>> trending({int limit = 50}) async {
    return _decode(
      () => RustLib.instance.api
          .crateFfiMarketplaceMarketplaceGetTrending(
              limit: limit, audience: 'public'),
    );
  }

  Future<ListingInfo> getListing(String listingId) async {
    try {
      final json = RustLib.instance.api
          .crateFfiMarketplaceMarketplaceGetListing(listingId: listingId);
      _current = ListingInfo.fromJson(jsonDecode(json));
      clearLastError();
      notifyDeferred();
      return _current!;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<String> createListing(
    String sellerPubkey,
    String title,
    String description,
    int price,
    String currency,
    String category,
    String condition,
    List<String> images,
    bool shippingAvailable,
  ) async {
    try {
      final eventId =
          RustLib.instance.api.crateFfiMarketplaceMarketplaceCreateListing(
        sellerPubkey: sellerPubkey,
        title: title,
        description: description,
        price: BigInt.from(price),
        currency: currency,
        category: category,
        condition: condition,
        imagesJson: jsonEncode(images),
        shippingAvailable: shippingAvailable,
      );
      clearLastError();
      return eventId;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<bool> updateListing(
    String listingId,
    String sellerPubkey,
    String title,
    String description,
    int price,
  ) async {
    try {
      final ok =
          RustLib.instance.api.crateFfiMarketplaceMarketplaceUpdateListing(
        listingId: listingId,
        sellerPubkey: sellerPubkey,
        title: title,
        description: description,
        price: BigInt.from(price),
      );
      clearLastError();
      notifyDeferred();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<bool> deleteListing(String listingId, String sellerPubkey) async {
    try {
      final ok =
          RustLib.instance.api.crateFfiMarketplaceMarketplaceDeleteListing(
        listingId: listingId,
        sellerPubkey: sellerPubkey,
      );
      clearLastError();
      notifyDeferred();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<String> createOrder(
    String listingId,
    String buyerPubkey,
    String sellerPubkey,
  ) async {
    try {
      final orderId =
          RustLib.instance.api.crateFfiMarketplaceMarketplaceCreateOrder(
        listingId: listingId,
        buyerPubkey: buyerPubkey,
        sellerPubkey: sellerPubkey,
      );
      clearLastError();
      return orderId;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<List<OrderInfo>> buyerOrders(String buyerPubkey) async {
    return _decodeOrders(
      () => RustLib.instance.api.crateFfiMarketplaceMarketplaceFetchBuyerOrders(
        buyerPubkey: buyerPubkey,
      ),
    );
  }

  /// Full order detail by id (fresh DB read, not the orders-tab cache).
  Future<OrderInfo> getOrder(String orderId) async {
    try {
      final json = RustLib.instance.api.crateFfiMarketplaceMarketplaceGetOrder(
        orderId: orderId,
      );
      final order =
          OrderInfo.fromJson(jsonDecode(json) as Map<String, dynamic>);
      clearLastError();
      return order;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  /// Full escrow detail by id (fresh DB read).
  Future<EscrowInfo> getEscrow(String escrowId) async {
    try {
      final json = RustLib.instance.api.crateFfiMarketplaceMarketplaceGetEscrow(
        escrowId: escrowId,
      );
      final decoded = jsonDecode(json);
      final map = decoded is List<dynamic>
          ? (decoded.isEmpty ? null : decoded.first as Map<String, dynamic>)
          : decoded as Map<String, dynamic>;
      if (map == null) throw Exception('Escrow not found');
      final escrow = EscrowInfo.fromJson(map);
      clearLastError();
      return escrow;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<List<OrderInfo>> sellerOrders(String sellerPubkey) async {
    return _decodeOrders(
      () =>
          RustLib.instance.api.crateFfiMarketplaceMarketplaceFetchSellerOrders(
        sellerPubkey: sellerPubkey,
      ),
    );
  }

  Future<String> createEscrow(
    String orderId,
    String buyerPubkey,
    String sellerPubkey,
    int amount,
  ) async {
    try {
      final escrowId =
          RustLib.instance.api.crateFfiMarketplaceMarketplaceCreateEscrow(
        orderId: orderId,
        buyerPubkey: buyerPubkey,
        sellerPubkey: sellerPubkey,
        amount: BigInt.from(amount),
      );
      clearLastError();
      return escrowId;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<bool> disputeEscrow(
      String escrowId, String disputerPubkey, String reason) async {
    try {
      final ok =
          RustLib.instance.api.crateFfiMarketplaceMarketplaceDisputeEscrow(
        escrowId: escrowId,
        disputerPubkey: disputerPubkey,
        reason: reason,
      );
      clearLastError();
      notifyDeferred();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<bool> confirmEscrowBuyer(String escrowId, String buyerPubkey) async {
    try {
      final ok =
          RustLib.instance.api.crateFfiMarketplaceMarketplaceEscrowConfirmBuyer(
        escrowId: escrowId,
        caller: buyerPubkey,
      );
      clearLastError();
      notifyDeferred();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<bool> confirmEscrowSeller(String escrowId, String sellerPubkey) async {
    try {
      final ok = RustLib.instance.api
          .crateFfiMarketplaceMarketplaceEscrowConfirmSeller(
        escrowId: escrowId,
        caller: sellerPubkey,
      );
      clearLastError();
      notifyDeferred();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<bool> releaseEscrow(String escrowId, String sellerPubkey) async {
    try {
      final ok =
          RustLib.instance.api.crateFfiMarketplaceMarketplaceReleaseEscrow(
        escrowId: escrowId,
        sellerPubkey: sellerPubkey,
      );
      clearLastError();
      notifyDeferred();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<bool> resolveEscrow(
    String escrowId,
    String mediatorPubkey,
    String winnerPubkey,
  ) async {
    try {
      final ok =
          RustLib.instance.api.crateFfiMarketplaceMarketplaceResolveEscrow(
        escrowId: escrowId,
        mediatorPubkey: mediatorPubkey,
        winnerPubkey: winnerPubkey,
      );
      clearLastError();
      notifyDeferred();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  /// Latest escrow for a listing (null when none exists yet).
  Future<EscrowInfo?> getEscrowByListing(String listingId) async {
    try {
      final json = RustLib.instance.api
          .crateFfiMarketplaceMarketplaceGetEscrowByListing(
              listingId: listingId);
      if (json.trim() == 'null') return null;
      return EscrowInfo.fromJson(jsonDecode(json) as Map<String, dynamic>);
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  /// Full parsed content JSON of a listing's kind-30402 post. Fields the
  /// bridge's ListingInfo mapping drops (geohash, tags, escrowEnabled, …)
  /// are read from here. Empty map when the row is missing or unparseable.
  Future<Map<String, dynamic>> listingContent(String listingId) async {
    try {
      final json =
          RustLib.instance.api.crateFfiMarketplaceMarketplaceGetContent(
        listingId: listingId,
      );
      final decoded = jsonDecode(json);
      return decoded is Map<String, dynamic> ? decoded : {};
    } catch (e) {
      return {};
    }
  }

  /// Whether a listing advertises escrow support. The flag lives in the
  /// listing's kind-30402 content JSON (`escrowEnabled`), which the bridge's
  /// ListingInfo mapping drops — so read it from the posts row directly.
  Future<bool> supportsEscrow(String listingId) async {
    final content = await listingContent(listingId);
    return content['escrowEnabled'] as bool? ?? false;
  }

  /// First locally-known order for a listing (orders tab cache).
  OrderInfo? orderForListing(String listingId) {
    for (final o in _orders) {
      if (o.listingId == listingId) return o;
    }
    return null;
  }

  Future<bool> reviewListing({
    required String listingId,
    required String reviewerPubkey,
    required int rating,
    String text = '',
  }) async {
    try {
      final ok =
          RustLib.instance.api.crateFfiMarketplaceMarketplaceReviewListing(
        listingId: listingId,
        reviewerPubkey: reviewerPubkey,
        rating: rating,
        text: text,
      );
      clearLastError();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      return false;
    }
  }

  Future<List<dynamic>> listingReviews(String listingId,
      {int limit = 10}) async {
    try {
      final json =
          RustLib.instance.api.crateFfiMarketplaceMarketplaceListingReviews(
        listingId: listingId,
        limit: limit,
      );
      final decoded = jsonDecode(json);
      clearLastError();
      return decoded as List<dynamic>? ?? [];
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      return [];
    }
  }

  Future<double> listingRating(String listingId) async {
    try {
      final rating =
          RustLib.instance.api.crateFfiMarketplaceMarketplaceListingRating(
        listingId: listingId,
      );
      clearLastError();
      return rating;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      return 0.0;
    }
  }

  Future<Map<String, dynamic>?> pollCreate({
    required String userPubkey,
    required String question,
    required String optionsJson,
    int expiresInHours = 168,
  }) async {
    try {
      final json =
          RustLib.instance.api.crateFfiMarketplaceMarketplacePollCreate(
        userPubkey: userPubkey,
        question: question,
        optionsJson: optionsJson,
        expiresInHours: expiresInHours,
      );
      final decoded = jsonDecode(json);
      clearLastError();
      return decoded is Map<String, dynamic> ? decoded : null;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      return null;
    }
  }

  Future<bool> pollVote({
    required String pollId,
    required String voterPubkey,
    required int optionIndex,
  }) async {
    try {
      final ok = RustLib.instance.api.crateFfiMarketplaceMarketplacePollVote(
        pollId: pollId,
        voterPubkey: voterPubkey,
        optionIndex: optionIndex,
      );
      clearLastError();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      return false;
    }
  }

  Future<bool> pollClose(String pollId, String userPubkey) async {
    try {
      final ok = RustLib.instance.api.crateFfiMarketplaceMarketplacePollClose(
        pollId: pollId,
        userPubkey: userPubkey,
      );
      clearLastError();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      return false;
    }
  }

  Future<Map<String, dynamic>?> pollGet(String pollId) async {
    try {
      final json = RustLib.instance.api.crateFfiMarketplaceMarketplacePollGet(
        pollId: pollId,
      );
      final decoded = jsonDecode(json);
      clearLastError();
      return decoded is Map<String, dynamic> ? decoded : null;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      return null;
    }
  }

  Future<bool> pollHasVoted(String pollId, String voterPubkey) async {
    try {
      final ok =
          RustLib.instance.api.crateFfiMarketplaceMarketplacePollHasVoted(
        pollId: pollId,
        voterPubkey: voterPubkey,
      );
      clearLastError();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      return false;
    }
  }

  Future<List<ListingInfo>> _decode(String Function() call) async {
    try {
      final json = call();
      final parsed = await runOffThread(() => _parseListings(json));
      _listings = parsed.length > 100 ? parsed.sublist(0, 100) : parsed;
      _listingsLoading = false;
      clearLastError();
      notifyDeferred();
      return _listings;
    } catch (e, st) {
      setLastError(e, st);
      _listingsLoading = false;
      notifyDeferred();
      rethrow;
    }
  }

  Future<List<OrderInfo>> _decodeOrders(String Function() call) async {
    try {
      final json = call();
      _orders = await runOffThread(() => _parseOrders(json));
      clearLastError();
      notifyDeferred();
      return _orders;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  /// Escrow rows where `pubkey` participates as buyer or seller (JSON).
  Future<List<EscrowInfo>> escrowsByParticipant(String pubkey) async {
    try {
      final json = RustLib.instance.api
          .crateFfiDbDbGetEscrowsByParticipant(pubkey: pubkey);
      clearLastError();
      return await runOffThread(() => _parseEscrows(json));
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }
}

/// Marketplace listing.
class ListingInfo {
  final String id;
  final String sellerPubkey;
  final String sellerName;
  final String title;
  final String description;
  final List<String> images;
  final int price;
  final String currency;
  final String category;
  final String condition;
  final bool shippingAvailable;
  final int createdAt;
  final int updatedAt;
  final String status;

  ListingInfo({
    required this.id,
    required this.sellerPubkey,
    required this.sellerName,
    required this.title,
    required this.description,
    required this.images,
    required this.price,
    required this.currency,
    required this.category,
    required this.condition,
    required this.shippingAvailable,
    required this.createdAt,
    required this.updatedAt,
    required this.status,
  });

  factory ListingInfo.fromJson(Map<String, dynamic> json) {
    return ListingInfo(
      id: json.strOf('id'),
      sellerPubkey: json.strOf('seller_pubkey'),
      sellerName: json.strOf('seller_name'),
      title: json.strOf('title'),
      description: json.strOf('description'),
      images: (json['images'] as List<dynamic>? ?? [])
          .map((e) => e.toString())
          .toList(),
      price: json.intOf('price'),
      currency: json.strOf('currency'),
      category: json.strOf('category'),
      condition: json.strOf('condition'),
      shippingAvailable: json.boolOf('shipping_available'),
      createdAt: json.intOf('created_at'),
      updatedAt: json.intOf('updated_at'),
      status: json.strOf('status'),
    );
  }

  String get priceLabel => '$price ${currency.isEmpty ? 'sat' : currency}';
}

/// Marketplace order.
class OrderInfo {
  final String id;
  final String listingId;
  final String buyerPubkey;
  final String sellerPubkey;
  final String status;
  final int amount;
  final int createdAt;

  OrderInfo({
    required this.id,
    required this.listingId,
    required this.buyerPubkey,
    required this.sellerPubkey,
    required this.status,
    required this.amount,
    required this.createdAt,
  });

  factory OrderInfo.fromJson(Map<String, dynamic> json) {
    return OrderInfo(
      id: json.strOf('id'),
      listingId: json.strOf('listing_id'),
      buyerPubkey: json.strOf('buyer_pubkey'),
      sellerPubkey: json.strOf('seller_pubkey'),
      status: json.strOf('status'),
      amount: json.intOf('amount'),
      createdAt: json.intOf('created_at'),
    );
  }
}

/// Marketplace escrow (state machine: created/disputed/completed/refunded).
class EscrowInfo {
  final String id;
  final String listingId;
  final String buyerPubkey;
  final String sellerPubkey;
  final int amountMsats;
  final String currency;
  final String status;
  final String note;
  final int createdAt;
  final int updatedAt;

  EscrowInfo({
    required this.id,
    required this.listingId,
    required this.buyerPubkey,
    required this.sellerPubkey,
    required this.amountMsats,
    required this.currency,
    required this.status,
    required this.note,
    required this.createdAt,
    required this.updatedAt,
  });

  factory EscrowInfo.fromJson(Map<String, dynamic> json) {
    return EscrowInfo(
      id: json.strOf('id'),
      listingId: json.strOf('listing_id'),
      buyerPubkey: json.strOf('buyer_pubkey'),
      sellerPubkey: json.strOf('seller_pubkey'),
      amountMsats: json.intOf('amount_msats'),
      currency: json.strOf('currency'),
      status: json.strOf('status'),
      note: json.strOf('escrow_note'),
      createdAt: json.intOf('created_at'),
      updatedAt: json.intOf('updated_at'),
    );
  }

  bool get isTerminal => status == 'completed' || status == 'refunded';
}

/// JSON → [ListingInfo] list, top-level so [runOffThread] can decode on a
/// background isolate.
List<ListingInfo> _parseListings(String json) {
  final decoded = jsonDecode(json);
  return (decoded as List<dynamic>)
      .map((e) => ListingInfo.fromJson(e as Map<String, dynamic>))
      .toList();
}

/// JSON → [OrderInfo] list, top-level so [runOffThread] can decode on a
/// background isolate.
List<OrderInfo> _parseOrders(String json) {
  final decoded = jsonDecode(json);
  return (decoded as List<dynamic>)
      .map((e) => OrderInfo.fromJson(e as Map<String, dynamic>))
      .toList();
}

/// JSON → [EscrowInfo] list, top-level so [runOffThread] can decode on a
/// background isolate.
List<EscrowInfo> _parseEscrows(String json) {
  final decoded = jsonDecode(json);
  return (decoded as List<dynamic>)
      .map((e) => EscrowInfo.fromJson(e as Map<String, dynamic>))
      .toList();
}
