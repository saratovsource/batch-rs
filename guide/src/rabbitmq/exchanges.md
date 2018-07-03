# Exchanges

When you're using RabbitMQ, an exchange is the place where you will publish your messages. Before being able to use an exchange, you must first declare it:

```rust
extern crate batch;

use batch::rabbitmq::exchanges;

exchanges! {
	Example {
		name = "batch.example"
	}
}
```
