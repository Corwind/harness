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
            // Box the URLSessionTask so onTermination can cancel it explicitly.
            let urlTaskBox = URLTaskBox()

            let task = Task {
                let bytes: URLSession.AsyncBytes
                let response: URLResponse
                do {
                    (bytes, response) = try await session.bytes(for: request)
                    urlTaskBox.task = bytes.task
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
                            // Strip trailing CR for CRLF.
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
                    // Flush any trailing event the server didn't terminate.
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

            continuation.onTermination = { _ in
                urlTaskBox.task?.cancel()
                task.cancel()
            }
        }
    }

    private final class URLTaskBox: @unchecked Sendable {
        var task: URLSessionDataTask?
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
