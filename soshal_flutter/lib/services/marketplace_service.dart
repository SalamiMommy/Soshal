// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';

/// Marketplace Service
/// NIP-15 style listings, orders and escrow through the bridge.
class MarketplaceService extends ChangeNotifier with LastErrorMixin {
  List<ListingInfo> _listings = [];
  ListingInfo? _current;
  List<OrderInfo> _orders = [];

  List<ListingInfo> get listings => _listings;
  ListingInfo? get current => _current;
  List<OrderInfo> get orders => _orders;

  Future<List<ListingInfo>> fetchListings(
      {int limit = 50, int offset = 0}) async {
    return _decode(
      () => RustLib.instance.api.crateFfiMarketplaceMarketplaceFetchListings(
        limit: limit,
        offset: offset,
      ),
    );
  }

  Future<List<ListingInfo>> search(String query, {int limit = 50}) async {
    return _decode(
      () => RustLib.instance.api.crateFfiMarketplaceMarketplaceSearch(
        query: query,
        limit: limit,
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
      ),
    );
  }

  Future<List<ListingInfo>> trending({int limit = 50}) async {
    return _decode(
      () => RustLib.instance.api
          .crateFfiMarketplaceMarketplaceGetTrending(limit: limit),
    );
  }

  Future<ListingInfo> getListing(String listingId) async {
    try {
      final json = RustLib.instance.api
          .crateFfiMarketplaceMarketplaceGetListing(listingId: listingId);
      _current = ListingInfo.fromJson(jsonDecode(json));
      _lastError = null;
      notifyListeners();
      return _current!;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
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
      _lastError = null;
      return eventId;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
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
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
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
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
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
      _lastError = null;
      return orderId;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
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
      _lastError = null;
      return escrowId;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
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
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
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
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
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
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
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
      notifyListeners();
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

  Future<List<ListingInfo>> _decode(String Function() call) async {
    try {
      final json = call();
      final decoded = jsonDecode(json);
      final parsed = (decoded as List<dynamic>)
          .map((e) => ListingInfo.fromJson(e as Map<String, dynamic>))
          .toList();
      _listings = parsed.length > 100 ? parsed.sublist(0, 100) : parsed;
      _lastError = null;
      notifyListeners();
      return _listings;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  Future<List<OrderInfo>> _decodeOrders(String Function() call) async {
    try {
      final json = call();
      final decoded = jsonDecode(json);
      _orders = (decoded as List<dynamic>)
          .map((e) => OrderInfo.fromJson(e as Map<String, dynamic>))
          .toList();
      _lastError = null;
      notifyListeners();
      return _orders;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
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
      id: json['id'] as String? ?? '',
      sellerPubkey: json['seller_pubkey'] as String? ?? '',
      sellerName: json['seller_name'] as String? ?? '',
      title: json['title'] as String? ?? '',
      description: json['description'] as String? ?? '',
      images: (json['images'] as List<dynamic>? ?? [])
          .map((e) => e.toString())
          .toList(),
      price: (json['price'] as num?)?.toInt() ?? 0,
      currency: json['currency'] as String? ?? '',
      category: json['category'] as String? ?? '',
      condition: json['condition'] as String? ?? '',
      shippingAvailable: json['shipping_available'] as bool? ?? false,
      createdAt: (json['created_at'] as num?)?.toInt() ?? 0,
      updatedAt: (json['updated_at'] as num?)?.toInt() ?? 0,
      status: json['status'] as String? ?? '',
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
      id: json['id'] as String? ?? '',
      listingId: json['listing_id'] as String? ?? '',
      buyerPubkey: json['buyer_pubkey'] as String? ?? '',
      sellerPubkey: json['seller_pubkey'] as String? ?? '',
      status: json['status'] as String? ?? '',
      amount: (json['amount'] as num?)?.toInt() ?? 0,
      createdAt: (json['created_at'] as num?)?.toInt() ?? 0,
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
      id: json['id'] as String? ?? '',
      listingId: json['listing_id'] as String? ?? '',
      buyerPubkey: json['buyer_pubkey'] as String? ?? '',
      sellerPubkey: json['seller_pubkey'] as String? ?? '',
      amountMsats: (json['amount_msats'] as num?)?.toInt() ?? 0,
      currency: json['currency'] as String? ?? '',
      status: json['status'] as String? ?? '',
      note: json['escrow_note'] as String? ?? '',
      createdAt: (json['created_at'] as num?)?.toInt() ?? 0,
      updatedAt: (json['updated_at'] as num?)?.toInt() ?? 0,
    );
  }

  bool get isTerminal => status == 'completed' || status == 'refunded';
}
