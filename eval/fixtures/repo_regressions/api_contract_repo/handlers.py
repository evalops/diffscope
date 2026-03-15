def serialize_order(order):
    status = "shipped" if order.shipped_at else "pending"
    return {"id": order.id, "status": status}
