import Foundation

/// Parses Server-Sent Events from a `URLSession.bytes` stream into typed
/// `RunEvent` values. Cancelling iteration of the returned
/// `AsyncThrowingStream` cancels the underlying URLSessionDataTask.
public final class SSEReader: @unchecked Sendable {
    private let session: URLSession
    private let decoder: JSONDecoder

    public init(session: URLSession = .shared) {
        self.session = session
        self.decoder = JSONDecoder()
    }

    public func stream(request: URLRequest) -> AsyncThrowingStream<RunEvent, Error> {
        let session = self.session
        let decoder = self.decoder
        return AsyncThrowingStream { continuation in
            // Holds the URLSessionDataTask so cancellation can hang up the
            // socket from any path: producer task cancel, consumer task cancel
            // (via onTermination), or a withTaskCancellationHandler tripped
            // while we're suspended inside `bytes(for:)` or `for try await`.
            let urlTaskBox = URLTaskBox()

            let producer = Task {
                await Self.run(
                    request: request,
                    session: session,
                    decoder: decoder,
                    urlTaskBox: urlTaskBox,
                    continuation: continuation
                )
            }

            // Fires when the consumer drops the iterator OR the consumer Task
            // is cancelled while suspended on `next()`. Either way: hang up
            // the socket and cancel the producer so it stops decoding bytes.
            continuation.onTermination = { _ in
                urlTaskBox.task?.cancel()
                producer.cancel()
            }
        }
    }

    /// Runs the SSE pipeline. Wrapped in `withTaskCancellationHandler` so that
    /// cancellation of *this* task (the producer) immediately cancels the
    /// underlying URLSessionDataTask — which in turn forces the `bytes`
    /// AsyncSequence to throw, freeing the loop without waiting on URLSession's
    /// idle/retry timeouts (~90s+ chain in production).
    private static func run(
        request: URLRequest,
        session: URLSession,
        decoder: JSONDecoder,
        urlTaskBox: URLTaskBox,
        continuation: AsyncThrowingStream<RunEvent, Error>.Continuation
    ) async {
        await withTaskCancellationHandler {
            await runInner(
                request: request,
                session: session,
                decoder: decoder,
                urlTaskBox: urlTaskBox,
                continuation: continuation
            )
        } onCancel: {
            // Synchronous, no `await` allowed — just trip the data task.
            urlTaskBox.task?.cancel()
        }
    }

    private static func runInner(
        request: URLRequest,
        session: URLSession,
        decoder: JSONDecoder,
        urlTaskBox: URLTaskBox,
        continuation: AsyncThrowingStream<RunEvent, Error>.Continuation
    ) async {
        let bytes: URLSession.AsyncBytes
        let response: URLResponse
        do {
            (bytes, response) = try await session.bytes(for: request)
            // Capture the data task synchronously so any subsequent
            // cancellation can hang up the socket immediately.
            urlTaskBox.task = bytes.task
            // Race: cancellation may have fired between assigning
            // `urlTaskBox.task` and the cancellation handler reading it.
            // Re-check explicitly.
            if Task.isCancelled {
                bytes.task.cancel()
                continuation.finish(throwing: BackendError.cancelled)
                return
            }
        } catch is CancellationError {
            continuation.finish(throwing: BackendError.cancelled)
            return
        } catch let urlError as URLError where urlError.code == .cancelled {
            continuation.finish(throwing: BackendError.cancelled)
            return
        } catch {
            continuation.finish(throwing: BackendError.transport(String(describing: error)))
            return
        }

        if let http = response as? HTTPURLResponse, !(200..<300).contains(http.statusCode) {
            var data = Data()
            do {
                for try await byte in bytes {
                    data.append(byte)
                    if data.count > 64 * 1024 { break }
                }
            } catch {
                // ignore — we already have a status to surface
            }
            let body = try? decoder.decode(APIErrorBody.self, from: data)
            continuation.finish(throwing: BackendError.httpStatus(http.statusCode, body: body))
            return
        }

        var eventName: String?
        var dataLines: [String] = []

        func flush() {
            defer {
                eventName = nil
                dataLines.removeAll(keepingCapacity: true)
            }
            guard let name = eventName, !dataLines.isEmpty else { return }
            let payload = dataLines.joined(separator: "\n")
            guard let payloadData = payload.data(using: .utf8) else { return }
            do {
                let event = try Self.decode(name: name, data: payloadData, decoder: decoder)
                continuation.yield(event)
            } catch {
                continuation.finish(throwing: error)
            }
        }

        func handle(line: String) {
            if line.isEmpty {
                flush()
                return
            }
            if line.hasPrefix(":") { return } // comment / heartbeat
            if let colon = line.firstIndex(of: ":") {
                let field = String(line[..<colon])
                var valueStart = line.index(after: colon)
                if valueStart < line.endIndex, line[valueStart] == " " {
                    valueStart = line.index(after: valueStart)
                }
                let value = String(line[valueStart...])
                switch field {
                case "event": eventName = value
                case "data": dataLines.append(value)
                case "id", "retry": break
                default: break
                }
            }
        }

        do {
            var buffer: [UInt8] = []
            for try await byte in bytes {
                if Task.isCancelled {
                    continuation.finish(throwing: BackendError.cancelled)
                    return
                }
                if byte == 0x0A { // '\n'
                    if buffer.last == 0x0D { buffer.removeLast() }
                    let line = String(decoding: buffer, as: UTF8.self)
                    buffer.removeAll(keepingCapacity: true)
                    handle(line: line)
                } else {
                    buffer.append(byte)
                }
            }
            if !buffer.isEmpty {
                let line = String(decoding: buffer, as: UTF8.self)
                handle(line: line)
            }
            flush()
            continuation.finish()
        } catch is CancellationError {
            continuation.finish(throwing: BackendError.cancelled)
        } catch let urlError as URLError where urlError.code == .cancelled {
            continuation.finish(throwing: BackendError.cancelled)
        } catch {
            continuation.finish(throwing: BackendError.transport(String(describing: error)))
        }
    }

    private final class URLTaskBox: @unchecked Sendable {
        private let lock = NSLock()
        private var _task: URLSessionDataTask?
        var task: URLSessionDataTask? {
            get { lock.lock(); defer { lock.unlock() }; return _task }
            set { lock.lock(); defer { lock.unlock() }; _task = newValue }
        }
    }

    static func decode(name: String, data: Data, decoder: JSONDecoder) throws -> RunEvent {
        do {
            switch name {
            case "run.start":
                return .runStart(try decoder.decode(RunStartPayload.self, from: data))
            case "message.start":
                return .messageStart(try decoder.decode(MessageStartPayload.self, from: data))
            case "content.delta":
                return .contentDelta(try decoder.decode(ContentDeltaPayload.self, from: data))
            case "tool_use.start":
                return .toolUseStart(try decoder.decode(ToolUseStartPayload.self, from: data))
            case "tool_use.delta":
                return .toolUseDelta(try decoder.decode(ToolUseDeltaPayload.self, from: data))
            case "tool_use.stop":
                return .toolUseStop(try decoder.decode(ToolUseStopPayload.self, from: data))
            case "tool.start":
                return .toolStart(try decoder.decode(ToolStartPayload.self, from: data))
            case "tool.stdout":
                return .toolStdout(try decoder.decode(ToolChunkPayload.self, from: data))
            case "tool.stderr":
                return .toolStderr(try decoder.decode(ToolChunkPayload.self, from: data))
            case "tool.finish":
                return .toolFinish(try decoder.decode(ToolFinishPayload.self, from: data))
            case "tool.error":
                return .toolError(try decoder.decode(ToolErrorPayload.self, from: data))
            case "message.stop":
                return .messageStop(try decoder.decode(MessageStopPayload.self, from: data))
            case "error":
                return .error(try decoder.decode(ErrorPayload.self, from: data))
            case "run.end":
                return .runEnd(try decoder.decode(RunEndPayload.self, from: data))
            default:
                throw BackendError.malformedEvent("unknown event name: \(name)")
            }
        } catch let backend as BackendError {
            throw backend
        } catch {
            throw BackendError.malformedEvent("\(name): \(error)")
        }
    }
}
