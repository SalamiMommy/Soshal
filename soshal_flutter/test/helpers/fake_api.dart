import 'dart:convert';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'package:soshal_flutter/ffi/feed.dart';
import 'package:soshal_flutter/ffi/messaging.dart';
import 'package:soshal_flutter/ffi/media.dart' show DecodedImageRgbaDto;
import 'package:soshal_flutter/ffi/network.dart' show HttpResponseDto;
import 'package:soshal_flutter/ffi/p2p.dart' show P2pSwarmStatusDto;
import 'package:soshal_flutter/ffi/raster.dart' show ImpellerFrameBufferInfo;



/// Handler for a single FFI method. Receives the raw [Invocation] so stubs
/// can read named arguments.
typedef ApiHandler = dynamic Function(Invocation invocation);

/// Fake implementation of the flutter_rust_bridge API surface.
///
/// Every unimplemented member routes through [noSuchMethod] — tests register
/// handlers keyed by method name (e.g. `crateFfiFeedFeedFetchEvents`) and can
/// assert on recorded [calls]. No native library is loaded.
class FakeApi extends RustLibApi {
  final Map<Symbol, ApiHandler> handlers = {};
  final List<Invocation> calls = [];

  static final Map<Symbol, ApiHandler> _defaultHandlers = {
    #crateFfiSessionSessionSave: (_) => true,
    #crateFfiMessagingMessagingStoreDms: (_) => true,
    #crateFfiMessagingMessagingWatchDms: (_) => const Stream<String>.empty(),
    #crateFfiMessagingMessagingWatchConversations: (_) =>
        const Stream<List<String>>.empty(),
    #crateFfiFeedFeedWatchEvents: (_) => const Stream<String>.empty(),
    #crateFfiFeedFeedWatchEventsTyped: (_) =>
        const Stream<List<FeedPostDto>>.empty(),
    #crateFfiFeedFeedFetchEventsTyped: (_) =>
        Future<List<FeedPostDto>>.value([]),
    #crateFfiFeedFeedWatchThread: (_) => const Stream<String>.empty(),
    #crateFfiIdentityIdentityWatchProfile: (_) => const Stream<String>.empty(),
    #crateFfiNotificationsNotificationsWatch: (_) =>
        const Stream<String>.empty(),
    #crateFfiDbDbWatchSetting: (_) => const Stream<String>.empty(),
    #crateFfiGroupsGroupsWatchGroups: (_) => const Stream<String>.empty(),
    #crateFfiMarketplaceMarketplaceWatchListings: (_) =>
        const Stream<String>.empty(),
    #crateFfiDatingDatingWatchProfiles: (_) => const Stream<String>.empty(),
    #crateFfiMinisMinisWatch: (_) => const Stream<String>.empty(),
    #crateFfiMusicMusicSaved: (_) => '[]',
    #crateFfiMusicMusicPlaylistList: (_) => '[]',
    #crateFfiMinisMinisSaved: (_) => '[]',
    #crateFfiGroupsGroupsRoomsReactions: (_) => '[]',
    #crateFfiGroupsGroupsRoomsReact: (_) => true,
  };


  void stub(String method, ApiHandler handler) {
    handlers[Symbol(method)] = handler;
  }

  void stubString(String method, String result) {
    stub(method, (_) => result);
  }

  void stubStringBuilder(String method, String Function(Invocation) result) {
    stub(method, (inv) => result(inv));
  }

  void stubBool(String method, bool result) {
    stub(method, (_) => result);
  }

  void stubInt(String method, int result) {
    stub(method, (_) => result);
  }

  void stubListString(String method, List<String> result) {
    stub(method, (_) => result);
  }

  void stubStream<T>(String method, Stream<T> stream) {
    stub(method, (_) => stream);
  }

  void stubValue<T>(String method, T value) {
    stub(method, (_) => value);
  }

  void stubFuture<T>(String method, T value) {
    stub(method, (_) => Future<T>.value(value));
  }

  int callCount(String method) =>
      calls.where((c) => c.memberName == Symbol(method)).length;

  List<Invocation> callsOf(String method) =>
      calls.where((c) => c.memberName == Symbol(method)).toList();

  dynamic namedArg(Invocation invocation, String name) =>
      invocation.namedArguments[Symbol(name)];


  @override
  Future<String> crateFfiAuthAuthRestoreFromMnemonic({required String mnemonic, required String passphrase}) =>
      _asyncCall<String>('crateFfiAuthAuthRestoreFromMnemonic', [], {#mnemonic: mnemonic, #passphrase: passphrase});

  @override
  Future<String> crateFfiCallsCallsFetchSignals({required String myPubkey}) =>
      _asyncCall<String>('crateFfiCallsCallsFetchSignals', [], {#myPubkey: myPubkey});

  @override
  Future<String> crateFfiCallsCallsSendSignal({required String signalType, required String targetPubkey, required String callId, String? sdp, String? candidate, String? mediaType}) =>
      _asyncCall<String>('crateFfiCallsCallsSendSignal', [], {#signalType: signalType, #targetPubkey: targetPubkey, #callId: callId, #sdp: sdp, #candidate: candidate, #mediaType: mediaType});

  @override
  Future<String> crateFfiChatrandomChatrandomFetch({required String myPubkey, String? author, required BigInt limit}) =>
      _asyncCall<String>('crateFfiChatrandomChatrandomFetch', [], {#myPubkey: myPubkey, #author: author, #limit: limit});

  @override
  Future<String> crateFfiChatrandomChatrandomSend({required String requestType, required List<String> peers, required String contentJson}) =>
      _asyncCall<String>('crateFfiChatrandomChatrandomSend', [], {#requestType: requestType, #peers: peers, #contentJson: contentJson});

  @override
  Future<String> crateFfiCryptoCryptoFrostAggregateSignature({required String sharesJson, required int threshold, required String groupPubkey, required String messageHex}) =>
      _asyncCall<String>('crateFfiCryptoCryptoFrostAggregateSignature', [], {#sharesJson: sharesJson, #threshold: threshold, #groupPubkey: groupPubkey, #messageHex: messageHex});

  @override
  Future<String> crateFfiCryptoCryptoFrostGenerateJuryKeys({required int threshold, required int totalParticipants, required String groupPubkey}) =>
      _asyncCall<String>('crateFfiCryptoCryptoFrostGenerateJuryKeys', [], {#threshold: threshold, #totalParticipants: totalParticipants, #groupPubkey: groupPubkey});

  @override
  Future<String> crateFfiCryptoCryptoPirEvaluateQuery({required String queryJson, required List<String> recordHexList}) =>
      _asyncCall<String>('crateFfiCryptoCryptoPirEvaluateQuery', [], {#queryJson: queryJson, #recordHexList: recordHexList});

  @override
  Future<String> crateFfiCryptoCryptoPirGenerateQuery({required BigInt targetIndex, required BigInt dimension, required String clientPubkey}) =>
      _asyncCall<String>('crateFfiCryptoCryptoPirGenerateQuery', [], {#targetIndex: targetIndex, #dimension: dimension, #clientPubkey: clientPubkey});

  @override
  Future<String> crateFfiCryptoCryptoPqcKemDecaps({required String ciphertext, required String sk}) =>
      _asyncCall<String>('crateFfiCryptoCryptoPqcKemDecaps', [], {#ciphertext: ciphertext, #sk: sk});

  @override
  Future<String> crateFfiCryptoCryptoPqcKemEncaps({required String recipientPk}) =>
      _asyncCall<String>('crateFfiCryptoCryptoPqcKemEncaps', [], {#recipientPk: recipientPk});

  @override
  Future<String> crateFfiCryptoCryptoPqcKemKeygen() =>
      _asyncCall<String>('crateFfiCryptoCryptoPqcKemKeygen', [], {});

  @override
  Future<String> crateFfiFeedFeedCreateReaction({required String eventId, required String reactionType}) =>
      _asyncCall<String>('crateFfiFeedFeedCreateReaction', [], {#eventId: eventId, #reactionType: reactionType});

  @override
  Future<String> crateFfiFeedFeedDeletePost({required String eventId}) =>
      _asyncCall<String>('crateFfiFeedFeedDeletePost', [], {#eventId: eventId});

  @override
  Future<String> crateFfiFeedFeedPublishReply({required String content, required String rootEventId, required String replyToEventId}) =>
      _asyncCall<String>('crateFfiFeedFeedPublishReply', [], {#content: content, #rootEventId: rootEventId, #replyToEventId: replyToEventId});

  @override
  Future<String> crateFfiFeedFeedPublishTextNote({required String content, required String tagsJson}) =>
      _asyncCall<String>('crateFfiFeedFeedPublishTextNote', [], {#content: content, #tagsJson: tagsJson});

  @override
  Future<String> crateFfiFeedFeedRankPosts({required String eventsJson}) =>
      _asyncCall<String>('crateFfiFeedFeedRankPosts', [], {#eventsJson: eventsJson});

  @override
  Future<String> crateFfiFeedFeedFetchEvents({required String optionsJson}) =>
      _asyncCall<String>('crateFfiFeedFeedFetchEvents', [], {#optionsJson: optionsJson});

  @override
  Future<List<FeedPostDto>> crateFfiFeedFeedFetchEventsTyped({required FeedQueryOptions options}) {
    final invocation = Invocation.method(#crateFfiFeedFeedFetchEventsTyped, [], {#options: options});
    calls.add(invocation);
    if (handlers.containsKey(#crateFfiFeedFeedFetchEventsTyped)) {
      final res = handlers[#crateFfiFeedFeedFetchEventsTyped]!(invocation);
      if (res is Future) return res.then((val) => val as List<FeedPostDto>);
      return Future<List<FeedPostDto>>.value(res as List<FeedPostDto>);
    }
    if (handlers.containsKey(#crateFfiFeedFeedFetchEvents)) {
      final legacyOptionsJson = jsonEncode({
        'limit': options.limit,
        'offset': options.offset,
        'filter_type': options.filterType,
        if (options.cursorCreatedAt != null)
          'cursor_created_at': options.cursorCreatedAt!.toInt(),
        if (options.cursorId != null) 'cursor_id': options.cursorId,
        'audience': options.audience,
      });
      final legacyInv = Invocation.method(
        #crateFfiFeedFeedFetchEvents,
        [],
        {#optionsJson: legacyOptionsJson},
      );
      calls.add(legacyInv);
      final dynamic legacyResult = handlers[#crateFfiFeedFeedFetchEvents]!(legacyInv);
      if (legacyResult is Future) {
        return legacyResult.then((val) => _decodeLegacyFeedJsonToDtos(val as String));
      }
      return Future<List<FeedPostDto>>.value(
        _decodeLegacyFeedJsonToDtos(legacyResult as String),
      );
    }
    return Future<List<FeedPostDto>>.value(const []);
  }

  @override
  Stream<List<FeedPostDto>> crateFfiFeedFeedWatchEventsTyped({required FeedQueryOptions options}) {
    final invocation = Invocation.method(#crateFfiFeedFeedWatchEventsTyped, [], {#options: options});
    calls.add(invocation);
    if (handlers.containsKey(#crateFfiFeedFeedWatchEventsTyped)) {
      final res = handlers[#crateFfiFeedFeedWatchEventsTyped]!(invocation);
      return res as Stream<List<FeedPostDto>>;
    }
    if (handlers.containsKey(#crateFfiFeedFeedWatchEvents)) {
      final legacyOptionsJson = jsonEncode({
        'limit': options.limit,
        'offset': options.offset,
        'filter_type': options.filterType,
        'audience': options.audience,
      });
      final legacyInv = Invocation.method(
        #crateFfiFeedFeedWatchEvents,
        [],
        {#optionsJson: legacyOptionsJson},
      );
      calls.add(legacyInv);
      final stream = handlers[#crateFfiFeedFeedWatchEvents]!(legacyInv)
          as Stream<String>;
      return stream.map(_decodeLegacyFeedJsonToDtos);
    }
    return const Stream<List<FeedPostDto>>.empty();
  }

  @override
  Future<String> crateFfiFeedFeedFetchThread({required String eventId}) =>
      _asyncCall<String>('crateFfiFeedFeedFetchThread', [], {#eventId: eventId});

  @override
  Future<String> crateFfiFeedFeedFetchWindow(
          {required int startIndex, required int limit, required String audience}) =>
      _asyncCall<String>('crateFfiFeedFeedFetchWindow', [], {#startIndex: startIndex, #limit: limit, #audience: audience});

  @override
  Future<String> crateFfiEventsEventsFetchNearby(
          {required double latitude,
          required double longitude,
          required double radiusKm,
          required int limit,
          required String audience}) =>
      _asyncCall<String>('crateFfiEventsEventsFetchNearby', [], {#latitude: latitude, #longitude: longitude, #radiusKm: radiusKm, #limit: limit, #audience: audience});

  @override
  Future<String> crateFfiEventsEventsFetchUserEvents(
          {required String userPubkey, required int limit}) =>
      _asyncCall<String>('crateFfiEventsEventsFetchUserEvents', [], {#userPubkey: userPubkey, #limit: limit});

  @override
  Future<bool> crateFfiNetworkFreenetConnect({required String url, required String authToken}) =>
      _asyncCall<bool>('crateFfiNetworkFreenetConnect', [], {#url: url, #authToken: authToken});

  @override
  Future<String> crateFfiNetworkFreenetGetContract({required String url, required String authToken, required String key, required bool subscribe}) =>
      _asyncCall<String>('crateFfiNetworkFreenetGetContract', [], {#url: url, #authToken: authToken, #key: key, #subscribe: subscribe});

  @override
  Future<String> crateFfiNetworkFreenetPutContract({required String url, required String authToken, required String stateJson, required bool subscribe}) =>
      _asyncCall<String>('crateFfiNetworkFreenetPutContract', [], {#url: url, #authToken: authToken, #stateJson: stateJson, #subscribe: subscribe});

  @override
  Future<bool> crateFfiNetworkFreenetSubscribe({required String url, required String authToken, required String key, String? summaryJson}) =>
      _asyncCall<bool>('crateFfiNetworkFreenetSubscribe', [], {#url: url, #authToken: authToken, #key: key, #summaryJson: summaryJson});

  @override
  Future<String> crateFfiMediaMediaClearCache({required String cacheDir}) =>
      _asyncCall<String>('crateFfiMediaMediaClearCache', [], {#cacheDir: cacheDir});

  @override
  Future<String> crateFfiMediaMediaFetchBlob(
          {required String blobHash, required String outPath}) =>
      _asyncCall<String>('crateFfiMediaMediaFetchBlob', [], {#blobHash: blobHash, #outPath: outPath});

  @override
  Future<String> crateFfiMediaMediaUploadBlob({required List<int> data}) =>
      _asyncCall<String>('crateFfiMediaMediaUploadBlob', [], {#data: data});

  @override
  Future<DecodedImageRgbaDto> crateFfiMediaMediaDecodeImageRgba({required String filePathOrUrl, int? maxWidth, int? maxHeight}) =>
      _asyncCall<DecodedImageRgbaDto>('crateFfiMediaMediaDecodeImageRgba', [], {#filePathOrUrl: filePathOrUrl, #maxWidth: maxWidth, #maxHeight: maxHeight});

  @override
  Future<String> crateFfiMediaMediaFetch({required String url, required String cacheDir}) =>
      _asyncCall<String>('crateFfiMediaMediaFetch', [], {#url: url, #cacheDir: cacheDir});

  @override
  Future<String> crateFfiMediaMediaGetMimeType({required String filePath}) =>
      _asyncCall<String>('crateFfiMediaMediaGetMimeType', [], {#filePath: filePath});

  @override
  Future<Uint8List> crateFfiMediaMediaLoadLocal({required String filePath}) =>
      _asyncCall<Uint8List>('crateFfiMediaMediaLoadLocal', [], {#filePath: filePath});

  @override
  Future<String> crateFfiMediaMediaUpload({required String filePath, required String blossomServer}) =>
      _asyncCall<String>('crateFfiMediaMediaUpload', [], {#filePath: filePath, #blossomServer: blossomServer});

  @override
  Future<String> crateFfiMessagingMessagingSendDm({required String content, required String recipientPubkey}) =>
      _asyncCall<String>('crateFfiMessagingMessagingSendDm', [], {#content: content, #recipientPubkey: recipientPubkey});

  @override
  List<DirectMessageDto> crateFfiMessagingMessagingFetchDmsTyped({required String withPubkey, required int limit}) {
    final invocation = Invocation.method(#crateFfiMessagingMessagingFetchDmsTyped, [], {#withPubkey: withPubkey, #limit: limit});
    calls.add(invocation);
    if (handlers.containsKey(#crateFfiMessagingMessagingFetchDmsTyped)) {
      final res = handlers[#crateFfiMessagingMessagingFetchDmsTyped]!(invocation);
      return res as List<DirectMessageDto>;
    }
    if (handlers.containsKey(#crateFfiMessagingMessagingFetchDms)) {
      final legacyInv = Invocation.method(
        #crateFfiMessagingMessagingFetchDms,
        [],
        {#withPubkey: withPubkey, #limit: limit},
      );
      calls.add(legacyInv);
      final dynamic legacyResult = handlers[#crateFfiMessagingMessagingFetchDms]!(legacyInv);
      if (legacyResult is String) {
        return _decodeLegacyDmJsonToDtos(legacyResult);
      }
    }
    return const [];
  }

  @override
  Stream<List<DirectMessageDto>> crateFfiMessagingMessagingWatchDmsTyped({required String withPubkey, required int limit}) {
    final invocation = Invocation.method(#crateFfiMessagingMessagingWatchDmsTyped, [], {#withPubkey: withPubkey, #limit: limit});
    calls.add(invocation);
    if (handlers.containsKey(#crateFfiMessagingMessagingWatchDmsTyped)) {
      final res = handlers[#crateFfiMessagingMessagingWatchDmsTyped]!(invocation);
      return res as Stream<List<DirectMessageDto>>;
    }
    if (handlers.containsKey(#crateFfiMessagingMessagingWatchDms)) {
      final legacyInv = Invocation.method(
        #crateFfiMessagingMessagingWatchDms,
        [],
        {#withPubkey: withPubkey, #limit: limit},
      );
      calls.add(legacyInv);
      final stream = handlers[#crateFfiMessagingMessagingWatchDms]!(legacyInv)
          as Stream<String>;
      return stream.map(_decodeLegacyDmJsonToDtos);
    }
    return const Stream<List<DirectMessageDto>>.empty();
  }

  @override
  Future<String> crateFfiMusicMusicComment({required int trackKind, required String trackPubkey, required String trackD, required String content}) =>
      _asyncCall<String>('crateFfiMusicMusicComment', [], {#trackKind: trackKind, #trackPubkey: trackPubkey, #trackD: trackD, #content: content});

  @override
  Future<String> crateFfiMusicMusicComments({required int trackKind, required String trackPubkey, required String trackD}) =>
      _asyncCall<String>('crateFfiMusicMusicComments', [], {#trackKind: trackKind, #trackPubkey: trackPubkey, #trackD: trackD});

  @override
  Future<String> crateFfiMusicMusicFetch(
          {required String audience, String? author, required BigInt limit}) =>
      _asyncCall<String>('crateFfiMusicMusicFetch', [], {#audience: audience, #author: author, #limit: limit});

  @override
  Future<String> crateFfiMusicMusicPublish({required String mediaSource, String? title, String? thumbnail, required List<String> hashtags, String? audience}) =>
      _asyncCall<String>('crateFfiMusicMusicPublish', [], {#mediaSource: mediaSource, #title: title, #thumbnail: thumbnail, #hashtags: hashtags, #audience: audience});

  @override
  Future<String> crateFfiMinisMinisPublish({required String mediaSource, String? textOverlay, String? thumbnail, String? audience}) =>
      _asyncCall<String>('crateFfiMinisMinisPublish', [], {#mediaSource: mediaSource, #textOverlay: textOverlay, #thumbnail: thumbnail, #audience: audience});

  @override
  Future<String> crateFfiMusicMusicShareToFeed({required String trackId, required String trackPubkey, required String trackD, required String message, required List<String> hashtags}) =>
      _asyncCall<String>('crateFfiMusicMusicShareToFeed', [], {#trackId: trackId, #trackPubkey: trackPubkey, #trackD: trackD, #message: message, #hashtags: hashtags});

  @override
  Future<bool> crateFfiNetworkNetworkAddRelay({required String url}) =>
      _asyncCall<bool>('crateFfiNetworkNetworkAddRelay', [], {#url: url});

  @override
  Future<HttpResponseDto> crateFfiNetworkNetworkFetchHttp3({required String url, required String method, required String headersJson, Uint8List? body}) =>
      _asyncCall<HttpResponseDto>('crateFfiNetworkNetworkFetchHttp3', [], {#url: url, #method: method, #headersJson: headersJson, #body: body});

  @override
  Future<bool> crateFfiNetworkNetworkFreenetStatus() =>
      _asyncCall<bool>('crateFfiNetworkNetworkFreenetStatus', [], {});

  @override
  Future<String> crateFfiNetworkNetworkGetMultiBearerStatus({required String ownPubkey}) =>
      _asyncCall<String>('crateFfiNetworkNetworkGetMultiBearerStatus', [], {#ownPubkey: ownPubkey});

  @override
  Future<String> crateFfiNetworkNetworkGetRelayStatus() =>
      _asyncCall<String>('crateFfiNetworkNetworkGetRelayStatus', [], {});

  @override
  Future<bool> crateFfiNetworkNetworkI2PStatus() =>
      _asyncCall<bool>('crateFfiNetworkNetworkI2PStatus', [], {});

  @override
  Future<String> crateFfiNetworkNetworkInitRelays({required List<String> relayUrls}) =>
      _asyncCall<String>('crateFfiNetworkNetworkInitRelays', [], {#relayUrls: relayUrls});

  @override
  Future<String?> crateFfiNetworkNetworkProcessBleBeacon({required String beacon, required String localRootHex, required String ownPubkey}) =>
      _asyncCall<String?>('crateFfiNetworkNetworkProcessBleBeacon', [], {#beacon: beacon, #localRootHex: localRootHex, #ownPubkey: ownPubkey});

  @override
  Future<int> crateFfiNetworkNetworkPublishEvent({required String eventJson}) =>
      _asyncCall<int>('crateFfiNetworkNetworkPublishEvent', [], {#eventJson: eventJson});

  @override
  Future<String> crateFfiNetworkNetworkQueryEvents({required String filterJson}) =>
      _asyncCall<String>('crateFfiNetworkNetworkQueryEvents', [], {#filterJson: filterJson});

  @override
  Future<String> crateFfiNetworkNetworkReconcileProllyTree({required String localKvJson, required String remoteRootHash}) =>
      _asyncCall<String>('crateFfiNetworkNetworkReconcileProllyTree', [], {#localKvJson: localKvJson, #remoteRootHash: remoteRootHash});

  @override
  Future<String> crateFfiNetworkNetworkRelayConnectionStatus() =>
      _asyncCall<String>('crateFfiNetworkNetworkRelayConnectionStatus', [], {});

  @override
  Future<bool> crateFfiNetworkNetworkRemoveRelay({required String url}) =>
      _asyncCall<bool>('crateFfiNetworkNetworkRemoveRelay', [], {#url: url});

  @override
  Future<String> crateFfiNetworkNetworkSubscribe({required String filterJson}) =>
      _asyncCall<String>('crateFfiNetworkNetworkSubscribe', [], {#filterJson: filterJson});

  @override
  Future<bool> crateFfiNetworkNetworkUnsubscribe({required String subscriptionId}) =>
      _asyncCall<bool>('crateFfiNetworkNetworkUnsubscribe', [], {#subscriptionId: subscriptionId});

  @override
  Future<bool> crateFfiNetworkNetworkVerifyZkWotProof({required String proofJson, required String expectedWotRoot, required String blacklistedNullifiersJson}) =>
      _asyncCall<bool>('crateFfiNetworkNetworkVerifyZkWotProof', [], {#proofJson: proofJson, #expectedWotRoot: expectedWotRoot, #blacklistedNullifiersJson: blacklistedNullifiersJson});

  @override
  Future<P2pSwarmStatusDto> crateFfiP2PP2PSwarmStatusDtoDefault() =>
      _asyncCall<P2pSwarmStatusDto>('crateFfiP2PP2PSwarmStatusDtoDefault', [], {});

  @override
  Future<String> crateFfiProtocolHandlerProtocolGetMetadata({required String scheme, required String host, required String path}) =>
      _asyncCall<String>('crateFfiProtocolHandlerProtocolGetMetadata', [], {#scheme: scheme, #host: host, #path: path});

  @override
  Future<Uint8List> crateFfiProtocolHandlerProtocolHandleRequest({required String scheme, required String host, required String path}) =>
      _asyncCall<Uint8List>('crateFfiProtocolHandlerProtocolHandleRequest', [], {#scheme: scheme, #host: host, #path: path});

  @override
  Future<ImpellerFrameBufferInfo> crateFfiRasterRasterAllocateFrameBuffer({required int width, required int height}) =>
      _asyncCall<ImpellerFrameBufferInfo>('crateFfiRasterRasterAllocateFrameBuffer', [], {#width: width, #height: height});

  @override
  Future<bool> crateFfiRasterRasterReleaseFrameBuffer({required BigInt ptrAddr}) =>
      _asyncCall<bool>('crateFfiRasterRasterReleaseFrameBuffer', [], {#ptrAddr: ptrAddr});

  @override
  Future<String> crateFfiGuestbookGuestbookAdd({required String profilePubkey, required String content}) =>
      _asyncCall<String>('crateFfiGuestbookGuestbookAdd', [], {#profilePubkey: profilePubkey, #content: content});

  @override
  Future<String> crateFfiGuestbookGuestbookApprove({required String entryId, required bool approved}) =>
      _asyncCall<String>('crateFfiGuestbookGuestbookApprove', [], {#entryId: entryId, #approved: approved});

  @override
  Future<String> crateFfiSearchSearchRemoteGlobal({required String query, required BigInt limit, required String relaysJson}) =>
      _asyncCall<String>('crateFfiSearchSearchRemoteGlobal', [], {#query: query, #limit: limit, #relaysJson: relaysJson});

  @override
  Future<String> crateFfiSyncSyncStart({required String relaysJson}) =>
      _asyncCall<String>('crateFfiSyncSyncStart', [], {#relaysJson: relaysJson});

  @override
  Future<bool> crateFfiSyncSyncStop() =>
      _asyncCall<bool>('crateFfiSyncSyncStop', [], {});

  @override
  Future<int> crateFfiHeadlessBackgroundSyncTask({required String dbPath}) =>
      _asyncCall<int>('crateFfiHeadlessBackgroundSyncTask', [], {#dbPath: dbPath});

  @override
  Future<String> crateFfiVouchVouchFetch({required String targetPubkey}) =>
      _asyncCall<String>('crateFfiVouchVouchFetch', [], {#targetPubkey: targetPubkey});

  @override
  Future<String> crateFfiVouchVouchPublish({required String targetPubkey, required String content}) =>
      _asyncCall<String>('crateFfiVouchVouchPublish', [], {#targetPubkey: targetPubkey, #content: content});

  @override
  Future<bool> crateFfiZapZapConnectNwc({required String nwcUri}) =>
      _asyncCall<bool>('crateFfiZapZapConnectNwc', [], {#nwcUri: nwcUri});

  @override
  Future<bool> crateFfiZapZapDisconnectNwc() =>
      _asyncCall<bool>('crateFfiZapZapDisconnectNwc', [], {});

  @override
  Future<String> crateFfiZapZapFetchInvoice({required String lnurl, required BigInt amountMsat, required String comment, required String nostrEvent}) =>
      _asyncCall<String>('crateFfiZapZapFetchInvoice', [], {#lnurl: lnurl, #amountMsat: amountMsat, #comment: comment, #nostrEvent: nostrEvent});

  @override
  Future<String> crateFfiZapZapFetchReceipts({required String eventId, required int limit}) =>
      _asyncCall<String>('crateFfiZapZapFetchReceipts', [], {#eventId: eventId, #limit: limit});

  @override
  Future<String> crateFfiZapZapGetNwcPubkey() =>
      _asyncCall<String>('crateFfiZapZapGetNwcPubkey', [], {});

  @override
  Future<String> crateFfiZapZapGetNwcStatus() =>
      _asyncCall<String>('crateFfiZapZapGetNwcStatus', [], {});

  @override
  Future<BigInt> crateFfiZapZapGetTotalMsat({required String eventId}) =>
      _asyncCall<BigInt>('crateFfiZapZapGetTotalMsat', [], {#eventId: eventId});

  @override
  Future<String> crateFfiZapZapParseLnurlMetadata({required String lnurl}) =>
      _asyncCall<String>('crateFfiZapZapParseLnurlMetadata', [], {#lnurl: lnurl});

  @override
  Future<String> crateFfiZapZapSendPayment({required String bolt11}) =>
      _asyncCall<String>('crateFfiZapZapSendPayment', [], {#bolt11: bolt11});

  @override
  Future<String> crateFfiAnalyticsAnalyticsSlmGenerateEmbedding({required String text}) =>
      _asyncCall<String>('crateFfiAnalyticsAnalyticsSlmGenerateEmbedding', [], {#text: text});

  @override
  Future<String> crateFfiAnalyticsAnalyticsComputeStats() =>
      _asyncCall<String>('crateFfiAnalyticsAnalyticsComputeStats', [], {});

  @override
  Future<String> crateFfiAnalyticsAnalyticsSlmClassifyPost({required String text}) =>
      _asyncCall<String>('crateFfiAnalyticsAnalyticsSlmClassifyPost', [], {#text: text});

  @override
  Future<String> crateFfiMediaMediaUploadBlobFile({required String filePath}) =>
      _asyncCall<String>('crateFfiMediaMediaUploadBlobFile', [], {#filePath: filePath});

  @override
  Future<bool> crateFfiPinPinSet({required String pin}) =>
      _asyncCall<bool>('crateFfiPinPinSet', [], {#pin: pin});

  @override
  Future<bool> crateFfiPinPinVerify({required String pin}) =>
      _asyncCall<bool>('crateFfiPinPinVerify', [], {#pin: pin});

  @override
  Future<Float32List> crateFfiStorageStorageGetAudioPeaks({required String path}) =>
      _asyncCall<Float32List>('crateFfiStorageStorageGetAudioPeaks', [], {#path: path});

  @override
  Future<Uint8List> crateFfiStorageStorageEncodeVoicePcm({required List<int> pcm}) =>
      _asyncCall<Uint8List>('crateFfiStorageStorageEncodeVoicePcm', [], {#pcm: pcm}).then((v) => Uint8List.fromList(v));

  @override
  Future<Uint8List> crateFfiRenderRenderComputeMeshFrame({required int sessionId, required String nodesJson, required double deltaTime}) =>
      _asyncCall<Uint8List>('crateFfiRenderRenderComputeMeshFrame', [], {#sessionId: sessionId, #nodesJson: nodesJson, #deltaTime: deltaTime}).then((v) => Uint8List.fromList(v));

  @override
  Future<bool> crateFfiSignerSignerSaveToKeyring({required String pubkey}) =>
      _asyncCall<bool>('crateFfiSignerSignerSaveToKeyring', [], {#pubkey: pubkey});

  @override
  Future<bool> crateFfiSignerSignerUnlockFromKeyring({required String pubkey}) =>
      _asyncCall<bool>('crateFfiSignerSignerUnlockFromKeyring', [], {#pubkey: pubkey});

  @override
  Future<Uint8List> crateFfiP2PP2PQuicFetchChunk({required String addr, required String hash, required BigInt offset, required BigInt length}) =>
      _asyncCall<Uint8List>('crateFfiP2PP2PQuicFetchChunk', [], {#addr: addr, #hash: hash, #offset: offset, #length: length}).then((v) => Uint8List.fromList(v));

  @override
  Future<String> crateFfiP2PP2PFetchBlobFromPeer({required String blobHash, required String ip, required int tcpPort, int? quicPort, required String outPath}) =>
      _asyncCall<String>('crateFfiP2PP2PFetchBlobFromPeer', [], {#blobHash: blobHash, #ip: ip, #tcpPort: tcpPort, #quicPort: quicPort, #outPath: outPath});

  @override
  Future<String> crateFfiP2PP2PMoqSubscribeFetch({required String addr, required String streamId, required BigInt windowMs}) =>
      _asyncCall<String>('crateFfiP2PP2PMoqSubscribeFetch', [], {#addr: addr, #streamId: streamId, #windowMs: windowMs});

  /// Typed async bridge call: records the [Invocation], dispatches to the
  /// registered stub and coerces the result to [T]. Deliberately NOT async:
  /// synchronous throws from stubs must propagate synchronously through the
  /// service's try/catch (tests rely on that timing).
  Future<T> _asyncCall<T>(
    String method,
    List<Object?> positional,
    Map<Symbol, Object?> named,
  ) {
    final invocation = Invocation.method(Symbol(method), positional, named);
    calls.add(invocation);
    final handler = handlers[Symbol(method)];
    if (handler == null) {
      throw UnimplementedError('no FakeApi stub registered for $method');
    }
    final result = handler(invocation);
    if (result is Future) return result.then((value) => value as T);
    return Future<T>.value(result as T);
  }

  static List<FeedPostDto> _decodeLegacyFeedJsonToDtos(String jsonStr) {
    if (jsonStr.isEmpty) return const [];
    try {
      final decoded = jsonDecode(jsonStr);
      if (decoded is! List) return const [];
      final list = <FeedPostDto>[];
      for (final item in decoded) {
        if (item is! Map) continue;
        final map = item.cast<String, dynamic>();
        PostMediaDto? media;
        if (map['media'] is Map) {
          final m = (map['media'] as Map).cast<String, dynamic>();
          media = PostMediaDto(
            url: m['url'] as String? ?? '',
            mediaType: (m['type'] ?? m['mediaType'] ?? 'image') as String,
            blobHash: (m['blob_hash'] ?? m['blobHash'] ?? '') as String,
            size: PlatformInt64Util.from((m['size'] as num? ?? 0).toInt()),
          );
        }
        list.add(FeedPostDto(
          eventId: (map['event_id'] ?? map['id'] ?? '') as String,
          pubkey: map['pubkey'] as String? ?? '',
          content: map['content'] as String? ?? '',
          createdAt: BigInt.from((map['created_at'] as num? ?? 0).toInt()),
          reactions: (map['reactions'] as num? ?? 0).toInt(),
          replies: (map['replies'] as num? ?? 0).toInt(),
          reposts: (map['reposts'] as num? ?? 0).toInt(),
          liked: map['liked'] as bool? ?? false,
          profileName: map['profile_name'] as String?,
          profilePicture: map['profile_picture'] as String?,
          media: media,
        ));
      }
      return list;
    } catch (_) {
      return const [];
    }
  }

  static List<DirectMessageDto> _decodeLegacyDmJsonToDtos(String jsonStr) {
    if (jsonStr.isEmpty) return const [];
    try {
      final decoded = jsonDecode(jsonStr);
      if (decoded is! List) return const [];
      final list = <DirectMessageDto>[];
      for (final item in decoded) {
        if (item is! Map) continue;
        final map = item.cast<String, dynamic>();
        list.add(DirectMessageDto(
          id: (map['id'] ?? '') as String,
          sender: (map['sender'] ?? '') as String,
          recipient: (map['recipient'] ?? '') as String,
          content: (map['content'] ?? '') as String,
          createdAt: BigInt.from((map['created_at'] as num? ?? 0).toInt()),
          decrypted: map['decrypted'] as bool? ?? true,
          isOwn: map['is_own'] as bool? ?? false,
          tags: (map['tags'] ?? map['tagsJson'] ?? '') as String,
        ));
      }
      return list;
    } catch (_) {
      return const [];
    }
  }

  @override
  dynamic noSuchMethod(Invocation invocation) {
    calls.add(invocation);
    final handler =
        handlers[invocation.memberName] ?? _defaultHandlers[invocation.memberName];
    if (handler != null) {
      final res = handler(invocation);
      final defaultVal = _defaultHandlers[invocation.memberName]?.call(invocation);
      if (defaultVal is Future && res is! Future) {
        return Future.value(res);
      }
      return res;
    }
    throw UnimplementedError(
      'no FakeApi stub registered for ${invocation.memberName}',
    );
  }
}

/// Installs [api] as the singleton flutter_rust_bridge api. Must be called
/// once per test file — the RustLib singleton is per-isolate and
/// [RustLib.initMock] throws if called twice in the same isolate.
void installFakeApi(FakeApi api) {
  RustLib.initMock(api: api);
}